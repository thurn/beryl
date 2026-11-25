//! Resolving the top of the stack (see CR 608), and transactional replay for bodies that pause
//! (architecture §7.3).
//!
//! [CR 405.5](https://yawgatog.com/resources/magic-rules/#R4055): "When all players pass in
//! succession, the top (last-added) spell or ability on the stack resolves."
//!
//! An ability that may ask a question ([`Ability::asks`]) runs on the scratch copy of the
//! state with the answers recorded so far. If it finishes, the copy becomes the state. If it
//! asks a question the record cannot answer, the copy is discarded and the question waits in
//! `pending` with the record, over the unchanged pre-resolution state; the answer is appended
//! and it runs again from the start. An inline answer source runs every body once on the
//! state itself.
//!
//! Implements: CR 608.2b, CR 608.2m, CR 608.2n, CR 608.3, CR 702.102d

use crate::bitboard::Bitboard;
use beryl_card::definition::Shape;

use crate::card::{Ability, AbilityKind, CastingOption, Face, FaceIx, Stop};
use crate::casting;
use crate::chance;
use crate::decision::{Answers, Choice, Decision, Outcome, StallReason};
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::{Layout, Queue, index};
use crate::observation::Observation;
use crate::replacement::event::{self, Entry, Event, Reason, Sink, Tagged};
use crate::resolution::Resolution;
use crate::scratch::Scratch;
use crate::state::{
    CounterKind, Pending, PendingTrigger, StackEntry, StackItem, State, TargetPlaces, Zone,
    hidden_pick_answer, placed,
};
use crate::table::GameTable;
use crate::{interpret, replacement};

/// Resolves the top of the stack, or continues a paused resolution with its record.
///
/// While a replacement effect's choice can come up ([`replacement::may_ask`]), as a dredge for a
/// card the body draws, every body runs on the copy, since any may meet it. So does every
/// resolution of a game with a card that chooses a value as it enters
/// ([`replacement::resolution_may_ask`]): a permanent spell, or a body that puts a permanent onto
/// the battlefield, may meet that choice. An inline source
/// then answers each question as the run stops at it, and the body runs again with the
/// answer recorded: the record holds a replacement's choice and the body's own questions in
/// the order they came, so each run asks them alike.
///
/// A resolution that finishes with no choice made since the state was last the same is a
/// loop of mandatory actions, and the game is a draw
/// (see [CR 104.4b](https://yawgatog.com/resources/magic-rules/#R1044b)): "If a game ...
/// somehow enters a "loop" of mandatory actions, repeating a sequence of events with no way
/// to stop, the game is a draw." [`State::note_resolution`] finds it.
pub(crate) fn resolve<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    resolve_top(s, t, x, a)?;
    if !matches!(s.pending(), Pending::Resolving(_) | Pending::Mulligans)
        && (!State::<L>::SUPPORTS_SUBGAMES || s.turn().number() != 0)
        && s.outcome().is_none()
        && s.note_resolution()
    {
        crate::sba::reveal_face_down(s, &mut x.events);
        s.end_game(Outcome::Draw);
    }
    Ok(())
}

/// [`resolve`] up to the loop check.
#[inline]
fn resolve_top<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    let entry = s.stack().last().expect("a stack entry to resolve").clone();
    let ability = entry.ability_in(t);
    let record = match s.pending() {
        Pending::Resolving(r) => r.record.clone(),
        Pending::None
        | Pending::PlayOrDraw { .. }
        | Pending::Mulligans
        | Pending::Discard
        | Pending::LegendRule { .. }
        | Pending::EnterPayingLife { .. }
        | Pending::DeclareAttackers { .. }
        | Pending::DeclareBlockers { .. }
        | Pending::CombatDamage { .. }
        | Pending::ExactCombatDamage(_)
        | Pending::Casting(_)
        | Pending::StepActions(_)
        | Pending::TurnStart(_)
        | Pending::TurningFaceUp { .. }
        | Pending::OrderEntered { .. }
        | Pending::EnterChoosing { .. }
        | Pending::ManaColor { .. }
        | Pending::Companion { .. }
        | Pending::CompanionPayment { .. } => Default::default(),
    };
    let may_ask = replacement::resolution_may_ask(t);
    // The source is asked first: whether an ability written as data asks is read from its
    // effect, which a rollout's inline source never needs.
    let may_play_subgame =
        State::<L>::SUPPORTS_SUBGAMES && ability.is_some_and(Ability::starts_subgame);
    if !may_ask && !may_play_subgame && (a.inline() || !ability.is_some_and(Ability::asks)) {
        // An inline source runs the body once on the committed state. A resolution that
        // paused under another source, as search's tree nodes do before a rollout takes over,
        // replays its record first, from what was known as it began.
        rewind(s, &mut x.events);
        let paused = run(
            s,
            t,
            &mut x.events,
            Some(a),
            Run {
                record: record.as_slice(),
                open: false,
            },
            &entry,
            ability,
        );
        unwind(&mut x.events);
        let paused = paused?;
        debug_assert!(
            paused.is_none() || s.outcome().is_some(),
            "a body paused outside a scratch copy"
        );
        return Ok(());
    }
    resolve_on_copy(s, t, x, a, &entry, ability, record)
}

/// [`resolve`] on the scratch copy: for a body that may ask while its source pauses, and for
/// every body while a replacement effect's choice can come up.
#[inline(never)]
fn resolve_on_copy<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    entry: &StackEntry<L>,
    ability: Option<&'static Ability>,
    mut record: L::Queue<L::Index, 32>,
) -> Result<(), Full> {
    let inline = a.inline();
    loop {
        let (spare, sink) = x.copy_to_spare(s);
        rewind(spare, sink);
        // Events of a run that ends paused are discarded with the copy (§7.3).
        let mark = sink.len();
        let answers = (!inline).then_some(&mut *a as &mut dyn Answers<L>);
        let open = Run {
            record: record.as_slice(),
            open: true,
        };
        let paused = run(spare, t, sink, answers, open, entry, ability);
        unwind(sink);
        let question = match paused {
            Ok(None) => {
                s.clone_from(spare);
                x.counts.copied();
                return Ok(());
            }
            Ok(Some(question)) => question,
            Err(full) => {
                sink.truncate(mark);
                return Err(full);
            }
        };
        sink.truncate(mark);
        let hidden = sink.asking.take_hidden();
        // Only an instruction that plays a subgame pauses with an engine-only question.
        if State::<L>::SUPPORTS_SUBGAMES && question.is_empty() {
            return crate::game::subgame::enter(s, spare, t, sink, record, question);
        }
        if inline {
            match answer_inline(s, spare, a, &question) {
                Some(c) => record.push(recorded(&question, c, hidden))?,
                None => return Ok(()),
            }
            continue;
        }
        keep_what_was_seen(s, spare, chance::replaying(sink));
        // While the question is open no one holds priority; the finished resolution gives
        // it to the active player (see CR 117.3b).
        s.reset_passes();
        s.pause_resolution(record, question, hidden);
        return Ok(());
    }
}

/// An inline source's answer to `question`, which a run on the copy `paused` stopped at, as
/// the index to record, counted toward the committed state `s`'s budget as a decision `next`
/// returned would be. `None` once the budget is spent: the game is then over, stalled.
pub(crate) fn answer_inline<L: Layout>(
    s: &mut State<L>,
    paused: &State<L>,
    a: &mut dyn Answers<L>,
    question: &Decision<L>,
) -> Option<L::Index> {
    if s.budgets().decisions >= s.profile().limits.decisions_per_turn {
        s.end_game(Outcome::Stalled(StallReason::DecisionsPerTurn));
        return None;
    }
    s.count_decision();
    if question.len() > 1 {
        s.note_choice();
    }
    // A body builds its questions for the player each belongs to; the player controlling
    // them answers (see CR 724.5, `engine::controlled`).
    let c = if s.turn().controller().is_some() {
        answer_controlled(s, paused, a, question)
    } else {
        a.answer(Observation::new(paused, question.player), question)
    };
    debug_assert!(c.is_some(), "an inline answer source declined to answer");
    Some(index::<L>(usize::from(c.unwrap_or(Choice(0)).0)))
}

/// The record's entry for the answer at index `c` to `question`: the index itself, or for a
/// hidden pick the slot it picked ([`crate::state::Resolving`], "Hidden picks").
pub(crate) fn recorded<L: Layout>(question: &Decision<L>, c: L::Index, hidden: bool) -> L::Index {
    if !hidden {
        return c;
    }
    let picked = crate::resolution::picked(question, Choice(c.into()));
    hidden_pick_answer::<L>(picked)
}

/// `a`'s answer to `question`, asked of the player controlling its player this turn.
#[cold]
#[inline(never)]
pub(crate) fn answer_controlled<L: Layout>(
    s: &State<L>,
    paused: &State<L>,
    a: &mut dyn Answers<L>,
    question: &Decision<L>,
) -> Option<Choice> {
    let asked = crate::engine::controlled(s, question.clone());
    a.answer(Observation::new(paused, asked.decider), &asked)
}

/// The answers a [`run`] starts from, and whether a question past them stops it
/// ([`crate::asking`]).
#[derive(Clone, Copy)]
struct Run<'a, L: Layout> {
    record: &'a [L::Index],
    open: bool,
}

/// Readies `s`, the committed state or a copy of it, to run a body from its start: with no
/// question open, with what players knew as the resolution began, which a paused resolution
/// keeps ([`State::resume_resolution`]), and with the cards its earlier runs revealed beyond
/// that in the sink, so chance does not count them as revealed again ([`chance`]). The
/// caller [`unwind`]s the sink once the run returns.
fn rewind<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>) {
    sink.revealed = s.resume_resolution();
}

/// Leaves the sink as it was before a run [`rewind`] readied it for: forgets the cards the
/// earlier runs revealed, and closes the scope [`run`] opened.
fn unwind<L: Layout>(sink: &mut Sink<L>) {
    sink.revealed = Bitboard::EMPTY;
    sink.leave();
}

/// What a paused run showed a player stays known in the committed state (architecture
/// §13): the cards a body looked at, revealed or drew before its question. A player who
/// looked at the top of their library knows those cards while they decide, so a state
/// determinized from what they know replays the body to the same question.
///
/// The runs themselves start from what was known as the resolution began
/// ([`State::resume_resolution`]). A card a run showed after one of its questions, as a discarded
/// card shown to the opponent, would otherwise be known before that question in the next run,
/// so the question would be asked with other candidates and its recorded index would name
/// another answer.
///
/// A slot keeps its identity through the moves of a run, so a card seen there is the card the
/// committed state holds in that slot, unless something random dealt identities anew (a
/// shuffle, a random order); then only a slot that is still the same object counts.
///
/// A run that replayed a save's record (`settled`) may have bound recorded cards to the slots
/// it revealed, trading identities among the cards no player knew ([`chance`]). With nothing
/// random dealt since, the committed state takes those identities, still unknown to every
/// player there, so the cards seen are the cards it holds and the run replays to the same
/// question.
pub(crate) fn keep_what_was_seen<L: Layout>(s: &mut State<L>, run: &State<L>, settled: bool) {
    let random = run.rng() != s.rng();
    if settled && !random {
        chance::adopt_unknown(s, run);
    }
    s.keep_seen(run, random);
}

/// Replays a paused resolution into the scratch state up to its question, so a viewer sees
/// the position the question is asked in (§7.3): the cards Brainstorm drew, for example. Its
/// events go to `seen` with their tags if given, and are not kept otherwise.
pub(crate) fn replay_to_question<'a, L: Layout>(
    s: &State<L>,
    t: &GameTable,
    x: &'a mut Scratch<L>,
    seen: Option<&mut Vec<Tagged<L>>>,
) -> Result<&'a State<L>, Full> {
    let Pending::Resolving(r) = s.pending().clone() else {
        unreachable!("a paused resolution");
    };
    let entry = s.stack().last().expect("a stack entry to resolve").clone();
    let ability = entry.ability_in(t);
    let (spare, sink) = x.copy_to_spare(s);
    rewind(spare, sink);
    let mark = sink.len();
    let open = Run {
        record: r.record.as_slice(),
        open: true,
    };
    let paused = run(spare, t, sink, None, open, &entry, ability);
    unwind(sink);
    if let Err(full) = paused {
        sink.truncate(mark);
        return Err(full);
    }
    match seen {
        Some(seen) => seen.extend(sink.take_from(mark)),
        None => sink.truncate(mark),
    }
    Ok(spare)
}

/// The targets of `entry` that are still legal as it begins to resolve, by their places in
/// the entry's list
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If the spell or ability
/// specifies targets, it checks whether the targets are still legal." Found once: whether it
/// resolves at all and what effect data may act on both read this.
pub(crate) fn legal_targets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
) -> TargetPlaces {
    let mut legal = TargetPlaces::NONE;
    for (place, target) in placed(entry.targets.as_slice()) {
        if casting::still_legal(s, t, entry, ability, target) {
            legal.insert(place);
        }
    }
    legal
}

/// Whether a triggered ability's intervening "if" no longer holds as it resolves
/// (see [CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "If the ability triggers,
/// it checks the stated condition again as it resolves. If the condition isn't true at that
/// time, the ability is removed from the stack and does nothing."
fn condition_fails<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
) -> bool {
    if ability.kind() != AbilityKind::Triggered || ability.condition().is_none() {
        return false;
    }
    let StackItem::Triggered { of, info } = entry.item else {
        unreachable!("a triggered ability's entry");
    };
    !interpret::condition_holds(s, t, &PendingTrigger { of, info }, ability)
}

/// Resolves `entry`, the top of `s`'s stack, answering from `answered`'s record and then
/// `answers`. Returns the question it paused at, if it did: one its body asked, or a
/// replacement effect's choice somewhere below it, which stops the run from any depth
/// ([`crate::asking`]).
///
/// Everything it does is the resolving spell's or ability's doing, down to its leaving the
/// stack ([`Cause::Effect`](crate::replacement::event::Cause::Effect)). It opens the sink's scope as it
/// takes the entry off the stack and returns with it open, however it ends: the caller
/// closes it ([`unwind`]), so that the question comes back through no frame of its own.
///
/// A run with no answers recorded that cannot stop, as nearly every resolution of a game is,
/// leaves the sink's record as it is between runs, closed and empty.
fn run<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    answers: Option<&mut dyn Answers<L>>,
    answered: Run<'_, L>,
    entry: &StackEntry<L>,
    ability: Option<&'static Ability>,
) -> Result<Option<Decision<L>>, Full> {
    if answered.open || !answered.record.is_empty() {
        return run_answered(s, t, sink, answers, answered, entry, ability);
    }
    s.take_top_of_stack();
    sink.enter_resolution(|| s.stack().len());
    body(s, t, sink, answers, entry, ability)
}

/// [`run`] with a record to answer from or a question to stop at.
#[inline(never)]
fn run_answered<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    answers: Option<&mut dyn Answers<L>>,
    answered: Run<'_, L>,
    entry: &StackEntry<L>,
    ability: Option<&'static Ability>,
) -> Result<Option<Decision<L>>, Full> {
    sink.asking.begin(answered.record, answered.open)?;
    s.take_top_of_stack();
    sink.enter_resolution(|| s.stack().len());
    let scopes = sink.save_scopes();
    let ran = body(s, t, sink, answers, entry, ability);
    let stopped = sink.asking.end();
    match ran {
        Err(Full) => {
            sink.restore_scopes(scopes);
            // Values and copies chosen for permanents of a batch that stopped before they
            // moved.
            sink.chosen.clear();
            sink.copies.clear();
            stopped.map_or(Err(Full), |question| Ok(Some(question)))
        }
        ran => ran,
    }
}

/// The body of [`run`], once the entry is off the stack.
fn body<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut answers: Option<&mut dyn Answers<L>>,
    entry: &StackEntry<L>,
    ability: Option<&'static Ability>,
) -> Result<Option<Decision<L>>, Full> {
    let def = t.def(entry.card());
    // CR 702.102d: "the controller of the spell follows the instructions of the
    // left half and then follows the instructions of the right half." A fused entry's
    // combined spell ability already puts the instructions in that order.
    let face = def.face(entry.face());
    // CR 608.2b: "If all its targets, for every instance of the word "target," are now
    // illegal, the spell or ability doesn't resolve." One that chose no targets is not
    // targeted (see CR 115.6) and resolves.
    let legal = ability.map_or(TargetPlaces::NONE, |ability| {
        legal_targets(s, t, entry, ability)
    });
    let spell = entry.spell();
    let mutating = spell.is_some() && mutating(face, entry);
    if mutating && legal.is_empty() {
        // see CR 702.140b: "As a mutating creature spell begins resolving, if its target is
        // illegal, it ceases to be a mutating creature spell and continues resolving as a
        // creature spell and will be put onto the battlefield under the control of the spell's
        // controller."
        enter_battlefield(s, t, sink, entry)?;
    } else if ability.is_some() && !entry.targets.is_empty() && legal.is_empty() {
        if spell.is_some() {
            casting::leave_stack(s, t, sink, entry, Zone::Graveyard, Reason::Resolve)?;
        }
    } else if spell.is_some() && !face.is_instant_or_sorcery() && !mutating {
        // An Aura's spell ability, which says what it targets, does its work by entering.
        if let Some(ability) = ability {
            crate::coverage::hit(ability);
        }
        enter_battlefield(s, t, sink, entry)?;
    } else if ability.is_some_and(|b| condition_fails(s, t, entry, b)) {
        // Removed from the stack; it does nothing.
    } else {
        // An instant or sorcery without a spell ability does nothing.
        if let Some(ability) = ability {
            let designation = ability.designation();
            if matches!(
                designation,
                beryl_card::Designation::LevelUp(_) | beryl_card::Designation::ToSolve
            ) {
                crate::objects::designation::resolve(s, t, sink, entry.source(), designation)?;
            }
            let answers = answers.as_mut().map(|a| &mut **a as &mut dyn Answers<L>);
            let mut resolution = Resolution::new(entry.clone(), ability, legal, answers);
            let mut game = Game::new(s, t, sink);
            crate::coverage::hit(ability);
            let flow = interpret::resolve(&mut game, &mut resolution);
            let question = resolution.question.take();
            let paused = interpret::finish(&mut game, flow)?;
            // See CR 725.1: this process differs "from the normal process for resolving
            // spells and abilities." The turn-ending instruction has handled the stack and
            // started cleanup. Commit the run without normal resolution's timing or moves.
            // See CR 728.1: "A game that is restarted immediately ends."
            if matches!(flow, Err(Stop::EndTurn | Stop::RestartGame)) || s.outcome().is_some() {
                return Ok(None);
            }
            if paused {
                return Ok(question);
            }
        }
        // CR 608.2n: "As the final part of an instant or sorcery spell's resolution, the spell
        // is put into its owner's graveyard. As the final part of an ability's resolution, the
        // ability is removed from the stack and ceases to exist." Unless its own effect moved
        // it already, as Time Spiral exiles itself.
        if let Some(spell) = spell.and_then(|spell| s.spell(spell)) {
            if entry.face() == FaceIx::BACK && def.shape == Shape::Adventure {
                casting::finish_adventure(s, t, sink, entry, spell)?;
            } else {
                casting::leave_stack(s, t, sink, entry, Zone::Graveyard, Reason::Resolve)?;
            }
        }
    }
    if spell.is_none() {
        sink.note(Event::AbilityFinished);
    }
    // The entry leaves the stack last, whatever became of a spell's card: one its own effect
    // moved resolved on without it
    // ([CR 608.2m](https://yawgatog.com/resources/magic-rules/#R6082m): "If an instant spell,
    // sorcery spell, or ability that can legally resolve leaves the stack once it starts to
    // resolve, it will continue to resolve fully.").
    sink.resolved();
    s.settle_captured_words();
    // see CR 117.3b: "The active player receives priority after a spell or ability (other than a
    // mana ability) resolves."
    s.give_priority(s.turn().active());
    Ok(None)
}

/// Whether `entry`, a spell of the face `face`, is a mutating creature spell: one cast for
/// its mutate cost (see [CR 702.140a]): "If you do, it becomes a mutating creature spell". Its
/// spell ability merges it with its target as it resolves ([`beryl_card::Effect::Mutate`]).
///
/// [CR 702.140a]: https://yawgatog.com/resources/magic-rules/#R702140a
fn mutating<L: Layout>(face: &Face, entry: &StackEntry<L>) -> bool {
    !face.casting.is_empty()
        && (entry.costs().paid(face)).any(|o| matches!(o, CastingOption::Mutate { .. }))
}

/// A permanent spell resolves, entering with what it was cast with: kicked, its offspring
/// cost or impending cost paid, and X.
///
/// [CR 608.3a](https://yawgatog.com/resources/magic-rules/#R6083a): "If the object that's
/// resolving has no targets, it becomes a permanent and enters the battlefield under the
/// control of the spell's controller." [CR 608.3c](https://yawgatog.com/resources/magic-rules/#R6083c):
/// "If the object that's resolving is an Aura spell, it becomes a permanent and is put onto the
/// battlefield under the control of the spell's controller attached to the player or object it
/// was targeting." see [CR 712.10](https://yawgatog.com/resources/magic-rules/#R71210): "a
/// resolving double-faced spell that becomes a permanent is put onto the battlefield with the
/// same face up that was face up on the stack."
fn enter_battlefield<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    entry: &StackEntry<L>,
) -> Result<(), Full> {
    let spell = entry.spell().expect("a spell's card");
    let face = t.def(entry.card()).face(entry.face());
    let mut how = Entry {
        controller: Some(entry.controller()),
        face: entry.face(),
        x: entry.x,
        // An Aura spell's target; a mutating creature spell entering as a creature attaches
        // to nothing.
        attach: if mutating(face, entry) {
            None
        } else {
            entry.targets.as_slice().first().and_then(|target| {
                use beryl_kernel::Attachment;
                target
                    .as_object()
                    .map(|r| Attachment::Object(r.slot))
                    .or_else(|| target.as_player().map(Attachment::Player))
            })
        },
        ..Entry::default()
    };
    for option in entry.costs().paid(face) {
        if let Some(kind) = option.kind() {
            how.paid = how.paid.with(kind);
        }
        if let CastingOption::Impending { counters, .. } = option {
            how.counters = Some((CounterKind::Time, counters));
        }
    }
    event::move_batch(
        s,
        t,
        sink,
        Bitboard::single(spell.slot),
        Zone::Battlefield,
        Reason::Resolve,
        how,
    )?;
    if s.has_prevention_shields() {
        s.carry_spell_shields(spell);
    }
    Ok(())
}
