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
use crate::event::{self, Entry, Reason, Sink};
use crate::fixed::{Fixed, Full};
use crate::ids::Seat;
use crate::state::{
    CounterKind, ObjectFlags, Pending, PendingTrigger, Resolving, StackEntry, State, Zone,
};
use crate::table::GameTable;

/// Resolves the top of the stack, or continues a paused resolution with its record.
pub(crate) fn resolve(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
) -> Result<(), Full> {
    let entry = *s.stack.as_slice().last().expect("a stack entry to resolve");
    let ability = entry_ability(t, &entry);
    if !ability.is_some_and(|b| b.may_pause) || a.inline() {
        let paused = run(s, t, &mut x.events, Some(a), &[], entry, ability)?;
        debug_assert!(
            paused.is_none() || s.outcome.is_some(),
            "a body paused outside a scratch copy"
        );
        return Ok(());
    }
    let record = match s.pending {
        Pending::Resolving(r) => r.record,
        _ => Fixed::new(),
    };
    let (spare, sink) = x.spare_and_sink();
    *spare = *s;
    spare.pending = Pending::None;
    // Events of a run that ends paused are discarded with the copy (§7.3).
    let mark = sink.len();
    match run(spare, t, sink, Some(a), record.as_slice(), entry, ability)? {
        None => *s = *spare,
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
fn keep_what_was_seen(s: &mut State, run: &State) {
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
/// events are not kept.
pub(crate) fn replay_to_question<'a>(
    s: &State,
    t: &GameTable,
    x: &'a mut Scratch,
) -> Result<&'a State, Full> {
    let Pending::Resolving(r) = s.pending else {
        unreachable!("a paused resolution");
    };
    let entry = *s.stack.as_slice().last().expect("a stack entry to resolve");
    let ability = entry_ability(t, &entry);
    let (spare, sink) = x.spare_and_sink();
    *spare = *s;
    spare.pending = Pending::None;
    let mark = sink.len();
    run(spare, t, sink, None, r.record.as_slice(), entry, ability)?;
    sink.truncate(mark);
    Ok(spare)
}

/// Whether every target of a spell or ability is illegal now, so it doesn't resolve
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If all its targets, for
/// every instance of the word "target," are now illegal, the spell or ability doesn't
/// resolve." One that chose no targets is not targeted (CR 115.6) and resolves.
fn fizzles(s: &State, t: &GameTable, entry: &StackEntry, ability: Option<&Ability>) -> bool {
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
fn condition_fails(s: &State, t: &GameTable, entry: &StackEntry, ability: &Ability) -> bool {
    if ability.kind != AbilityKind::Triggered {
        return false;
    }
    let Some(condition) = ability.condition else {
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
fn run(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    mut answers: Option<&mut dyn Answers>,
    record: &[u8],
    entry: StackEntry,
    ability: Option<&'static Ability>,
) -> Result<Option<Decision>, Full> {
    s.stack.pop();
    let face = &t.def(entry.card).faces[usize::from(entry.face)];
    if fizzles(s, t, &entry, ability) {
        if entry.is_spell() {
            cast::leave_stack(s, t, sink, &entry, false, Reason::Resolve)?;
        }
    } else if entry.is_spell() && !face.is_instant_or_sorcery() {
        enter_battlefield(s, t, sink, &entry)?;
    } else if ability.is_some_and(|b| condition_fails(s, t, &entry, b)) {
        // Removed from the stack; it does nothing.
    } else {
        let mut offer = None;
        // An instant or sorcery without a spell ability does nothing.
        if let Some(ability) = ability {
            let answers = answers.as_mut().map(|a| &mut **a as &mut dyn Answers);
            let mut ctx = EffectCtx::resolving(s, t, sink, answers, record, entry, ability);
            let flow = (ability.body)(&mut ctx);
            let question = ctx.question;
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
            cast::leave_stack(s, t, sink, &entry, false, Reason::Resolve)?;
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
fn enter_battlefield(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    entry: &StackEntry,
) -> Result<(), Full> {
    let slot = entry.object.expect("a spell's card").slot;
    let face = &t.def(entry.card).faces[usize::from(entry.face)];
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
    for (i, option) in face.casting.iter().enumerate() {
        if entry.costs & (1 << i) == 0 {
            continue;
        }
        match *option {
            CastingOption::Kicker { .. } => how.flags |= ObjectFlags::KICKED,
            CastingOption::Offspring(_) => how.flags |= ObjectFlags::OFFSPRING,
            CastingOption::Impending { counters, .. } => {
                how.flags |= ObjectFlags::IMPENDING;
                how.counters = Some((CounterKind::Time, counters));
            }
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
