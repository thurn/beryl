//! A step's turn-based actions that may pause at a replacement effect's choice (see [CR 616.1]),
//! as the draw step's draw may for a dredge.
//!
//! Implements: none. Replays turn-based actions when a replacement effect asks for a choice.
//!
//! The turn-based actions of a step are carried out as it begins, where no resolution is open
//! to pause. While such a choice can come up ([`crate::replacement::may_ask`]), the step begins on the
//! state itself, so players see it begin, and its turn-based actions run as a paused
//! resolution's body does (architecture §7.3): on the scratch copy of the state, with the
//! answers recorded so far. A question the record cannot answer discards the copy, and waits
//! in [`Pending::StepActions`] over the step as it began; its answer is recorded and the
//! actions run again from there. An inline source answers each question as the run stops at
//! it, and the actions run again with the answer recorded. With no such choice live, the step
//! ends and the next begins on the state itself, as before.
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use crate::bitboard::Bitboard;
use crate::decision::{Answers, Decision};
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{Sink, Tagged};
use crate::scratch::Scratch;
use crate::state::{Pending, State};
use crate::table::GameTable;
use crate::{chance, stack::resolve, turn};

/// Ends the current step and begins the next ([`turn::end_step`]), running the next step's
/// turn-based actions so that they can pause: the caller's way while a replacement effect's
/// choice can come up ([`crate::replacement::may_ask`]).
#[cold]
#[inline(never)]
pub(crate) fn end_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    if turn::end_step_before_actions(s, t, &mut x.events)? {
        step_actions(s, t, x, a)?;
    }
    Ok(())
}

/// Runs the turn-based actions of the step that has just begun on a copy of the state, or
/// runs them again with the record of a pause.
#[inline(never)]
pub(crate) fn step_actions<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    let mut record = match s.pending() {
        Pending::StepActions(r) => r.record.clone(),
        Pending::None
        | Pending::PlayOrDraw { .. }
        | Pending::Mulligans
        | Pending::Discard
        | Pending::LegendRule { .. }
        | Pending::EnterPayingLife { .. }
        | Pending::DeclareAttackers { .. }
        | Pending::DeclareBlockers { .. }
        | Pending::CombatDamage { .. }
        | Pending::Casting(_)
        | Pending::Resolving(_)
        | Pending::TurningFaceUp { .. }
        | Pending::OrderEntered { .. }
        | Pending::EnterChoosing { .. }
        | Pending::ManaColor { .. } => Default::default(),
    };
    let inline = a.inline();
    loop {
        let (spare, sink) = x.copy_to_spare(s);
        // Events of a run that ends paused are discarded with the copy.
        let mark = sink.len();
        let Some(question) = run(spare, t, sink, record.as_slice())? else {
            s.clone_from(spare);
            x.counts.copied();
            return Ok(());
        };
        sink.truncate(mark);
        if inline {
            match resolve::answer_inline(s, spare, a, &question) {
                Some(c) => record.push(c)?,
                None => return Ok(()),
            }
            continue;
        }
        resolve::keep_what_was_seen(s, spare, chance::replaying(sink));
        s.pause_step_actions(record, question);
        return Ok(());
    }
}

/// Replays a step's paused turn-based actions into the scratch state up to their question, so
/// a viewer sees the position the question is asked in. Their events go to `seen` with their
/// tags if given.
pub(crate) fn replay_to_question<'a, L: Layout>(
    s: &State<L>,
    t: &GameTable,
    x: &'a mut Scratch<L>,
    seen: Option<&mut Vec<Tagged<L>>>,
) -> Result<&'a State<L>, Full> {
    let Pending::StepActions(r) = s.pending().clone() else {
        unreachable!("a step's paused turn-based actions");
    };
    let (spare, sink) = x.copy_to_spare(s);
    let mark = sink.len();
    run(spare, t, sink, r.record.as_slice())?;
    match seen {
        Some(seen) => seen.extend(sink.take_from(mark)),
        None => sink.truncate(mark),
    }
    Ok(spare)
}

/// Runs the step's turn-based actions on `s`, a copy, answering from `record`. Returns the
/// question they stopped at, if they did ([`crate::asking`]).
///
/// They start from what each player knew as they began, as a paused resolution's runs do
/// ([`State::resume_resolution`]), so each run asks its questions with the same candidates.
fn run<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    record: &[L::Index],
) -> Result<Option<Decision<L>>, Full> {
    sink.revealed = s.resume_resolution();
    sink.asking.begin(record, true)?;
    let scopes = sink.save_scopes();
    let done = turn::step_actions(s, t, sink);
    let stopped = sink.asking.end();
    sink.revealed = Bitboard::EMPTY;
    match done {
        Err(Full) if stopped.is_some() => {
            sink.restore_scopes(scopes);
            // Values and copies chosen for permanents of a batch that stopped before they
            // moved.
            sink.chosen.clear();
            sink.copies.clear();
            Ok(stopped)
        }
        done => done.map(|()| None),
    }
}
