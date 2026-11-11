//! Resolving the top of the stack (CR 608), and transactional replay for bodies that pause
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

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityKind, CastingOption, FaceAbility};
use crate::cast::{self, entry_ability};
use crate::chance;
use crate::decision::{Answers, Decision};
use crate::effect::EffectCtx;
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::interpret;
use crate::layout::{Layout, Queue};
use crate::scratch::Scratch;
use crate::state::{
    CounterKind, ObjectFlags, Pending, PendingTrigger, Resolving, StackEntry, State, Zone,
};
use crate::table::GameTable;

/// Resolves the top of the stack, or continues a paused resolution with its record.
pub(crate) fn resolve<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    let entry = s
        .stack
        .as_slice()
        .last()
        .expect("a stack entry to resolve")
        .clone();
    let ability = entry_ability(t, &entry);
    let (record, knowledge) = match s.pending {
        Pending::Resolving(ref r) => (r.record.clone(), Some(r.knowledge)),
        _ => (Default::default(), None),
    };
    // The source is asked first: whether an ability written as data asks is read from its
    // effect, which a rollout's inline source never needs.
    if a.inline() || !ability.is_some_and(Ability::asks) {
        // An inline source runs the body once on the committed state. A resolution that
        // paused under another source, as search's tree nodes do before a rollout takes over,
        // replays its record first, from what was known as it began.
        s.pending = Pending::None;
        if let Some(knowledge) = knowledge {
            rewind(s, &mut x.events, knowledge);
        }
        let paused = run(
            s,
            t,
            &mut x.events,
            Some(a),
            record.as_slice(),
            &entry,
            ability,
        );
        x.events.revealed = Bitboard::EMPTY;
        let paused = paused?;
        debug_assert!(
            paused.is_none() || s.outcome.is_some(),
            "a body paused outside a scratch copy"
        );
        return Ok(());
    }
    let (spare, sink) = x.copy_to_spare(s);
    spare.pending = Pending::None;
    if let Some(knowledge) = knowledge {
        rewind(spare, sink, knowledge);
    }
    // Events of a run that ends paused are discarded with the copy (§7.3).
    let mark = sink.len();
    let paused = run(spare, t, sink, Some(a), record.as_slice(), &entry, ability);
    sink.revealed = Bitboard::EMPTY;
    match paused? {
        None => {
            s.clone_from(spare);
            x.counts.copied();
        }
        Some(question) => {
            sink.truncate(mark);
            keep_what_was_seen(s, spare, chance::replaying(sink));
            // While the question is open no one holds priority; the finished resolution gives
            // it to the active player (CR 117.3b).
            s.turn.passes = 0;
            s.pending = Pending::Resolving(Resolving {
                record,
                question,
                knowledge: knowledge.unwrap_or(s.knowledge),
            });
        }
    }
    Ok(())
}

/// Readies `s`, the committed state or a copy of it, to run a body from its start: with what
/// players knew as the resolution began, which a paused resolution keeps
/// ([`Resolving::knowledge`]), and with the cards its earlier runs revealed beyond that in the
/// sink, so chance does not count them as revealed again ([`chance`]). The caller empties
/// `sink.revealed` once the run returns.
fn rewind<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, knowledge: [Bitboard<L::Slot>; 2]) {
    sink.revealed = (s.knowledge[0] | s.knowledge[1]) & !(knowledge[0] | knowledge[1]);
    s.knowledge = knowledge;
}

/// What a paused run showed a player stays known in the committed state (architecture
/// §13): the cards a body looked at, revealed or drew before its question. A player who
/// looked at the top of their library knows those cards while they decide, so a state
/// determinized from what they know replays the body to the same question.
///
/// The runs themselves start from what was known as the resolution began
/// ([`Resolving::knowledge`]). A card a run showed after one of its questions, as a discarded
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
fn keep_what_was_seen<L: Layout>(s: &mut State<L>, run: &State<L>, settled: bool) {
    let random = run.rng != s.rng;
    if settled && !random {
        chance::adopt_unknown(s, run);
    }
    for p in Seat::BOTH {
        let gained = run.knowledge[p.index()] & !s.knowledge[p.index()];
        for slot in gained.iter() {
            let (now, then) = (s.object(slot), run.object(slot));
            if now.card == then.card && (!random || now.incarnation == then.incarnation) {
                s.knowledge[p.index()].insert(slot);
            }
        }
    }
}

/// Replays a paused resolution into the scratch state up to its question, so a viewer sees
/// the position the question is asked in (§7.3): the cards Brainstorm drew, for example. Its
/// events go to `seen` if given, and are not kept otherwise.
pub(crate) fn replay_to_question<'a, L: Layout>(
    s: &State<L>,
    t: &GameTable,
    x: &'a mut Scratch<L>,
    seen: Option<&mut Vec<Event<L>>>,
) -> Result<&'a State<L>, Full> {
    let Pending::Resolving(r) = s.pending.clone() else {
        unreachable!("a paused resolution");
    };
    let entry = s
        .stack
        .as_slice()
        .last()
        .expect("a stack entry to resolve")
        .clone();
    let ability = entry_ability(t, &entry);
    let (spare, sink) = x.copy_to_spare(s);
    spare.pending = Pending::None;
    rewind(spare, sink, r.knowledge);
    let mark = sink.len();
    let paused = run(spare, t, sink, None, r.record.as_slice(), &entry, ability);
    sink.revealed = Bitboard::EMPTY;
    paused?;
    match seen {
        Some(seen) => seen.extend(sink.take_from(mark)),
        None => sink.truncate(mark),
    }
    Ok(spare)
}

/// The targets of `entry` that are still legal as it begins to resolve, a bit for each by its
/// place in the entry's list
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If a spell or ability
/// specifies targets, it checks whether the targets are still legal." Found once: whether it
/// resolves at all and what effect data may act on both read this.
pub(crate) fn legal_targets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
) -> u8 {
    const _: () = assert!(crate::state::TARGETS <= 8, "a bit for each target");
    let mut legal = 0;
    for (i, &target) in entry.targets.iter().enumerate() {
        if cast::still_legal(s, t, entry, ability, target) {
            legal |= 1 << i;
        }
    }
    legal
}

/// Whether a triggered ability's intervening "if" no longer holds as it resolves
/// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "If the ability triggers,
/// it checks the stated condition again as it resolves. If the condition isn't true at that
/// time, the ability is removed from the stack and does nothing."
fn condition_fails<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
) -> bool {
    if ability.kind != AbilityKind::Triggered {
        return false;
    }
    if ability.condition.is_none() {
        return false;
    }
    let Some(FaceAbility::Own(index)) = entry.ability else {
        unreachable!("a triggered ability is one of its face's own");
    };
    let pending = PendingTrigger {
        source: entry.object,
        card: entry.card,
        face: entry.face,
        ability: index,
        controller: entry.controller,
        info: entry.info,
    };
    !interpret::intervening_if(s, t, &pending, ability)
}

/// Resolves `entry`, the top of `s`'s stack, answering from `record` and then `answers`.
/// Returns the question it paused at, if it did.
fn run<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut answers: Option<&mut dyn Answers<L>>,
    record: &[L::Index],
    entry: &StackEntry<L>,
    ability: Option<&'static Ability>,
) -> Result<Option<Decision<L>>, Full> {
    s.stack.pop();
    let face = t.def(entry.card).face(entry.face);
    // CR 608.2b: "If all its targets, for every instance of the word "target," are now
    // illegal, the spell or ability doesn't resolve." One that chose no targets is not
    // targeted (CR 115.6) and resolves.
    let legal = ability.map_or(0, |ability| legal_targets(s, t, entry, ability));
    if ability.is_some() && !entry.targets.is_empty() && legal == 0 {
        if entry.is_spell() {
            cast::leave_stack(s, t, sink, entry, false, Reason::Resolve)?;
        }
    } else if entry.is_spell() && !face.is_instant_or_sorcery() {
        // An Aura's spell ability, which says what it targets, does its work by entering.
        if let Some(ability) = ability {
            crate::coverage::hit(ability);
        }
        enter_battlefield(s, t, sink, entry)?;
    } else if ability.is_some_and(|b| condition_fails(s, t, entry, b)) {
        // Removed from the stack; it does nothing.
    } else {
        let mut offer = None;
        // An instant or sorcery without a spell ability does nothing.
        if let Some(ability) = ability {
            let answers = answers.as_mut().map(|a| &mut **a as &mut dyn Answers<L>);
            let mut ctx = EffectCtx::resolving(s, t, sink, answers, record, entry.clone(), ability);
            crate::coverage::hit(ability);
            let flow = interpret::resolve(&mut ctx, legal);
            let question = ctx.question.take();
            offer = ctx.offer;
            ctx.finish()?;
            if s.outcome.is_some() {
                return Ok(None);
            }
            if flow.is_err() {
                return Ok(question);
            }
        }
        // CR 608.2n: "As the final part of an instant or sorcery spell's resolution, the spell
        // is put into its owner's graveyard. As the final part of an ability's resolution, the
        // ability is removed from the stack and ceases to exist." Unless its own effect moved
        // it already, as Time Spiral exiles itself.
        if entry.is_spell() && entry.object.is_some_and(|r| s.is_current(r)) {
            cast::leave_stack(s, t, sink, entry, false, Reason::Resolve)?;
        }
        if let Some((object, player, free)) = offer {
            s.pending = Pending::CastOffer {
                object,
                player,
                free,
            };
        }
    }
    if !entry.is_spell() {
        sink.note(Event::AbilityFinished);
    }
    // CR 117.3b: "The active player receives priority after a spell or ability (other than a
    // mana ability) resolves."
    s.turn.priority = s.turn.active;
    s.turn.passes = 0;
    Ok(None)
}

/// A permanent spell resolves, entering with what it was cast with: kicked, its offspring
/// cost or impending cost paid, and X.
///
/// [CR 608.3a](https://yawgatog.com/resources/magic-rules/#R6083a): "If the object that's
/// resolving has no targets, it becomes a permanent and enters the battlefield under the
/// control of the spell's controller." [CR 608.3c](https://yawgatog.com/resources/magic-rules/#R6083c):
/// "If the object that's resolving is an Aura spell, it becomes a permanent and is put onto the
/// battlefield under the control of the spell's controller attached to the player or object it
/// was targeting." [CR 712.13](https://yawgatog.com/resources/magic-rules/#R71213): "a
/// resolving double-faced spell that becomes a permanent is put onto the battlefield with the
/// same face up that was face up on the stack."
fn enter_battlefield<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    entry: &StackEntry<L>,
) -> Result<(), Full> {
    let slot = entry.object.expect("a spell's card").slot;
    let face = t.def(entry.card).face(entry.face);
    let mut how = Entry {
        controller: Some(entry.controller),
        face: entry.face,
        x: entry.x,
        attach: entry
            .targets
            .as_slice()
            .first()
            .and_then(|target| target.as_object())
            .map(|r| r.slot),
        ..Entry::default()
    };
    for option in entry.costs.paid(face) {
        match option {
            CastingOption::Kicker { .. } => how.flags |= ObjectFlags::KICKED,
            CastingOption::Offspring(_) => how.flags |= ObjectFlags::OFFSPRING,
            CastingOption::Impending { counters, .. } => {
                how.flags |= ObjectFlags::IMPENDING;
                how.counters = Some((CounterKind::Time, counters));
            }
            CastingOption::Evoke { .. } => how.flags |= ObjectFlags::EVOKED,
            // An instant or sorcery's, which never enters.
            CastingOption::Overload { .. } => {}
        }
    }
    event::move_batch(
        s,
        t,
        sink,
        Bitboard::single(slot),
        Zone::Battlefield,
        Reason::Resolve,
        how,
    )
}
