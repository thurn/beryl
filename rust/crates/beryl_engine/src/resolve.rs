//! Resolving the top of the stack (CR 608), and transactional replay for bodies that pause
//! (architecture §7.3).
//!
//! [CR 405.5](https://yawgatog.com/resources/magic-rules/#R4055): "When all players pass in
//! succession, the top (last-added) spell or ability on the stack resolves."
//!
//! A body that declares `may_pause` runs on the scratch copy of the state with the answers
//! recorded so far. If it finishes, the copy becomes the state. If it asks a question the
//! record cannot answer, the copy is discarded and the question waits in `pending` with the
//! record, over the unchanged pre-resolution state; the answer is appended and the body runs
//! again from the start. An inline answer source runs every body once on the state itself.

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityKind, CastingOption, FaceAbility};
use crate::cast::{self, entry_ability};
use crate::decision::{Answers, Decision};
use crate::effect::EffectCtx;
use crate::engine::Scratch;
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
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
    let record = match s.pending {
        Pending::Resolving(ref r) => r.record.clone(),
        _ => Default::default(),
    };
    if !ability.is_some_and(|b| b.may_pause) || a.inline() {
        // An inline source runs the body once on the committed state. A resolution that
        // paused under another source, as search's tree nodes do before a rollout takes over,
        // replays its record first.
        s.pending = Pending::None;
        let paused = run(
            s,
            t,
            &mut x.events,
            Some(a),
            record.as_slice(),
            &entry,
            ability,
        )?;
        debug_assert!(
            paused.is_none() || s.outcome.is_some(),
            "a body paused outside a scratch copy"
        );
        return Ok(());
    }
    let (spare, sink) = x.copy_to_spare(s);
    spare.pending = Pending::None;
    // Events of a run that ends paused are discarded with the copy (§7.3).
    let mark = sink.len();
    match run(spare, t, sink, Some(a), record.as_slice(), &entry, ability)? {
        None => {
            s.clone_from(spare);
            x.counts.copied();
        }
        Some(question) => {
            sink.truncate(mark);
            keep_what_was_seen(s, spare);
            // While the question is open no one holds priority; the finished resolution gives
            // it to the active player (CR 117.3b).
            s.turn.passes = 0;
            s.pending = Pending::Resolving(Resolving { record, question });
        }
    }
    Ok(())
}

/// What a paused run showed a player stays known in the committed state (architecture
/// §13): the cards a body looked at, revealed or drew before its question. A player who
/// looked at the top of their library knows those cards while they decide, so a state
/// determinized from what they know replays the body to the same question.
///
/// A slot keeps its identity through the moves of a run, so a card seen there is the card the
/// committed state holds in that slot, unless something random dealt identities anew (a
/// shuffle, a random order); then only a slot that is still the same object counts.
fn keep_what_was_seen<L: Layout>(s: &mut State<L>, run: &State<L>) {
    let random = run.rng != s.rng;
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
    let mark = sink.len();
    run(spare, t, sink, None, r.record.as_slice(), &entry, ability)?;
    match seen {
        Some(seen) => seen.extend(sink.take_from(mark)),
        None => sink.truncate(mark),
    }
    Ok(spare)
}

/// Whether every target of a spell or ability is illegal now, so it doesn't resolve
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If all its targets, for
/// every instance of the word "target," are now illegal, the spell or ability doesn't
/// resolve." One that chose no targets is not targeted (CR 115.6) and resolves.
fn fizzles<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: Option<&Ability>,
) -> bool {
    let Some(ability) = ability else {
        return false;
    };
    !entry.targets.is_empty()
        && entry
            .targets
            .iter()
            .all(|&target| !cast::still_legal(s, t, entry, ability, target))
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
    let Some(condition) = L::condition(ability) else {
        return false;
    };
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
    !condition(s, t, &pending)
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
    if fizzles(s, t, entry, ability) {
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
            let flow = L::body(ability)(&mut ctx);
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
