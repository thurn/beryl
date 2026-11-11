//! The scripted agents that play a board's actions, and how far they have got.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use beryl_engine::card::Purpose;
use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Layout, Observation, Seat, TurnStep,
};
use beryl_scenario::{Action, Pick, Scenario};
use beryl_search::Agent;
use beryl_session::trigger_to_stack;
use std::fmt::Write as _;

/// How a board's scripts are going, shared by its two [`Scripted`] agents and the recording.
#[derive(Debug, Default)]
pub struct Progress {
    /// The first decision a script could not answer.
    pub failure: Option<String>,
    /// Each seat's next action, while any is left.
    pub next: [Option<Action>; 2],
    /// The last decision a seat's next action was due at and of the kind of, but fit no
    /// candidate of (a scripted target that resolves to none offered): why a script that
    /// never finishes stalled.
    pub missed: Option<String>,
    /// How many actions the two scripts have taken.
    pub taken: usize,
    /// A turn the scripts hold out for after their last action (a trailing
    /// [`Action::AdvanceTo`]), and the turn the game has reached, as its views show.
    pub until: Option<u16>,
    pub turn: u16,
}

impl Progress {
    /// Whether both scripts have taken all their actions, and the game has reached the turn
    /// they hold out for.
    pub fn done(&self) -> bool {
        self.next.iter().all(Option::is_none) && self.until.is_none_or(|t| self.turn >= t)
    }

    /// Why the scripts have not finished, if they have not: the actions still waiting, and
    /// the last decision one was due at that it fit no candidate of.
    pub fn stalled(&self) -> Option<String> {
        let waiting: Vec<&Action> = self.next.iter().flatten().collect();
        if waiting.is_empty() {
            return None;
        }
        let mut why = format!("the scripts waiting on {waiting:?}");
        if let Some(missed) = &self.missed {
            let _ = write!(why, "; {missed}");
        }
        Some(why)
    }
}

pub type Shared = Arc<Mutex<Progress>>;

/// The turn and step a script's action waits for, if any.
pub(super) type Gate = Option<(u16, TurnStep)>;

/// The progress of a board's scripts so far.
pub fn lock(progress: &Shared) -> std::sync::MutexGuard<'_, Progress> {
    progress.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An agent answering one seat's decisions from a board's actions for that seat, in order, by
/// the scenario vocabulary: each action answers the first decision it fits once the actions
/// listed before it, both seats', have been taken. Short of its next
/// action, the seat passes priority, pays by the first payment plan, orders simultaneous
/// triggers as the session orders the human's ([`trigger_to_stack`]), so a board plays the same
/// from either chair, and answers a question with one answer; any other decision is the
/// script's failure. Once its actions are taken,
/// it passes priority and takes the first candidate of anything else. Its view is the true
/// state: a script names the cards it means.
pub struct Scripted {
    table: GameTable,
    seat: Seat,
    /// The seat's actions, each with its place among both seats' actions and the turn and
    /// step it waits for, if any.
    script: VecDeque<(usize, Action, Gate)>,
    progress: Shared,
    /// Dropped with the agent, on its session's thread ([`Scripted::guarded`]).
    guard: Option<crate::panics::Guard>,
}

impl Scripted {
    /// `seat`'s part of `board`'s actions, played on `table`. Every action must answer one
    /// decision.
    pub fn new(
        board: &Scenario,
        seat: Seat,
        table: &GameTable,
        progress: Shared,
    ) -> Result<Scripted, String> {
        Scripted::with_actions(board.actions, seat, table, progress)
    }

    /// `seat`'s part of `actions`, played on `table`, as [`Scripted::new`] takes a board's. An
    /// [`Action::AdvanceTo`] holds the actions after it, both seats', until the game reaches its
    /// turn and step; it is not itself taken.
    pub fn with_actions<'a>(
        actions: impl IntoIterator<Item = &'a Action>,
        seat: Seat,
        table: &GameTable,
        progress: Shared,
    ) -> Result<Scripted, String> {
        let mut script = VecDeque::new();
        let mut gate = None;
        let mut at = 0;
        for action in actions {
            match *action {
                Action::PlayLand { player, .. }
                | Action::Cast { player, .. }
                | Action::Activate { player, .. }
                | Action::TurnFaceUp { player, .. }
                | Action::Answer { player, .. } => {
                    if player == seat {
                        script.push_back((at, *action, gate));
                    }
                    at += 1;
                }
                Action::AdvanceTo { turn, step } => gate = Some((turn, step)),
                _ => return Err(format!("{action:?}: a board's action answers one decision")),
            }
        }
        lock(&progress).next[seat.index()] = script.front().map(|&(_, a, _)| a);
        Ok(Scripted {
            table: table.clone(),
            seat,
            script,
            progress,
            guard: None,
        })
    }

    /// The agent holding `guard`, which tells of a panic on the session's thread that owns it.
    #[must_use]
    pub fn guarded(mut self, guard: crate::panics::Guard) -> Scripted {
        self.guard = Some(guard);
        self
    }
}

impl Scripted {
    /// Restore a Playground script after Undo by consuming the actions it had already
    /// taken at that decision. Use on a freshly constructed script so its turn gates remain.
    pub fn skip_taken(&mut self, count: usize) {
        for _ in 0..count {
            self.take();
        }
    }

    /// Takes the seat's next action: it is answered.
    fn take(&mut self) {
        self.script.pop_front();
        let mut progress = lock(&self.progress);
        progress.next[self.seat.index()] = self.script.front().map(|&(_, a, _)| a);
        progress.taken += 1;
    }

    /// A decision its script leaves to it short of `next`: discarding to hand size in a
    /// cleanup (the first cards), and combat (no attackers, no blockers, as a scenario's
    /// passing does).
    fn by_default<L: Layout>(
        &self,
        s: &beryl_engine::State<L>,
        d: &Decision<L>,
        next: Option<Action>,
    ) -> Option<Choice> {
        let scripted = matches!(next, Some(Action::Answer { kind, .. }) if kind == d.kind);
        if d.prompt.purpose == Purpose::Cleanup && !scripted {
            return Some(Choice(0));
        }
        if matches!(d.kind, DecisionKind::Attacker | DecisionKind::Blocker)
            && next.is_some()
            && !scripted
        {
            return beryl_scenario::answer(
                &self.table,
                s,
                d,
                &Action::answer(d.player, d.kind, Pick::Done),
            );
        }
        None
    }
}

impl<L: Layout> Agent<L> for Scripted {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Choice {
        let s = obs.unmasked_root();
        let next = self.script.front().map(|&(_, a, _)| a);
        // An action waits for the other seat's actions before it: a response is cast once
        // what it answers is on the stack. It waits for its turn and step too.
        let now = (s.turn().number(), s.turn().step());
        let due = self.script.front().is_some_and(|&(at, _, gate)| {
            lock(&self.progress).taken >= at && gate.is_none_or(|g| now >= g)
        });
        // A "may" its script answers that the session folds into the question it leads to (a
        // search's choice, with Done to find nothing): a yes is the choice itself, a no is Done.
        if let Some(Action::Answer {
            kind: DecisionKind::YesNo,
            pick,
            ..
        }) = next.filter(|_| due)
            && !matches!(d.kind, DecisionKind::YesNo | DecisionKind::Priority)
            && d.player == self.seat
        {
            let done = beryl_scenario::answer(
                &self.table,
                s,
                d,
                &Action::answer(d.player, d.kind, Pick::Done),
            );
            if let Some(done) = done {
                self.take();
                if matches!(pick, Pick::No) {
                    return done;
                }
                return self.choose(obs, d);
            }
        }
        // A target its script names that was the only one, chosen without asking: the seat is
        // back at priority with the spell or ability cast.
        if let Some(Action::Answer {
            kind: DecisionKind::Target,
            ..
        }) = next.filter(|_| due)
            && d.kind == DecisionKind::Priority
            && d.player == self.seat
        {
            self.take();
            return self.choose(obs, d);
        }
        if let Some(action) = next.filter(|_| due) {
            let kind = match action {
                Action::Answer { kind, .. } => kind,
                _ => DecisionKind::Priority,
            };
            if kind == d.kind {
                if let Some(c) = beryl_scenario::answer(&self.table, s, d, &action) {
                    self.take();
                    return c;
                }
                lock(&self.progress).missed = Some(format!(
                    "{action:?} fit no candidate of {}",
                    beryl_scenario::describe(&self.table, s, d),
                ));
            }
        }
        if d.kind == DecisionKind::Priority
            && let Some(c) = beryl_scenario::answer(&self.table, s, d, &Action::pass(d.player))
        {
            return c;
        }
        if d.kind == DecisionKind::TriggerOrder {
            return trigger_to_stack(s, &self.table, d);
        }
        if let Some(c) = self.by_default(s, d, next) {
            return c;
        }
        if let Some(action) = next
            && d.kind != DecisionKind::PaymentPlan
            && d.len() > 1
        {
            lock(&self.progress).failure.get_or_insert_with(|| {
                format!(
                    "unscripted decision: {}, where the board's next action is {action:?}",
                    beryl_scenario::describe(&self.table, s, d),
                )
            });
        }
        Choice(0)
    }
}
