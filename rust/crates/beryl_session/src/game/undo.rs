//! Taking back the human's answers: undo points, and the cast in progress that can be cancelled.

use beryl_engine::state::{ObjectRef, StackEntry};
use beryl_engine::{Large, Relayout, Small, TurnStep, paused_events};
use beryl_search::{Agent, AgentDecision};

use crate::SessionError;
use crate::protocol::{PlayerView, StopsSetting};
use crate::replay::Entry;

use super::{At, Boards, Game, Open, Snapshot, on_board, with_stops};

/// The game as it stood at the human's priority before they began to cast a spell or activate
/// an ability, kept until its costs are paid so that they can take it back ([`Game::cancel`]).
#[derive(Clone)]
pub(super) struct BeforeCast {
    pub(super) state: Snapshot,
    pub(super) at: At,
    pub(super) answered: Option<Vec<StackEntry<Large>>>,
    /// The length of the choice log then.
    pub(super) log: usize,
    pub(super) stops: StopsSetting,
    /// How many undo points had been kept by then, counting those dropped since
    /// ([`Game::undo_depth`]).
    pub(super) undo: usize,
}

/// The most undo points a game keeps ([`Game::undo`]), the oldest dropped first: at most
/// 20 KiB each in [`Small`], so ten megabytes at most, and more than a whole game's answers
/// under the smart stops.
pub const UNDO_POINTS: usize = 500;

/// The game as it stood at one of the human's decisions or halts before they answered it,
/// which they may go back to ([`Game::undo`]). A copy of the state, random streams and all, so
/// going back is exact: the library and every random stream are as they were, so the cards
/// drawn since come out again in the same order, and a shuffle falls the same way.
pub(super) struct UndoPoint {
    pub(super) state: Snapshot,
    pub(super) continuation: bool,
    /// The open decision there.
    pub(super) at: At,
    pub(super) seen: (u16, TurnStep),
    pub(super) answered: Option<Vec<StackEntry<Large>>>,
    pub(super) before_cast: Option<BeforeCast>,
    /// The agent's next decision among its own then ([`Agent::next_decision`]), which it goes
    /// back to.
    pub(super) agent: Option<AgentDecision>,
    pub(super) hurt_by: [Option<ObjectRef<Large>>; 2],
}

impl Game {
    /// Whether the human's open decision is part of a spell they are casting or an ability
    /// they are activating from their priority, whose costs are not yet paid: they may take
    /// it back ([`Game::cancel`]).
    pub(super) fn cancellable(&self) -> bool {
        self.before_cast.is_some()
            && matches!(self.at, At::Human(_, Open::Decision(_)))
            && self.casting()
    }

    /// Whether a spell is being cast or an ability activated, short of being paid for: not a
    /// triggered ability's targets being chosen, which the engine asks the same way once a
    /// cast is complete ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d):
    /// "The remainder of the process for putting a triggered ability on the stack is
    /// identical to the process for casting a spell").
    pub(super) fn casting(&self) -> bool {
        self.board.casting() == Some(false)
    }

    /// Takes back the spell the human is casting or the ability they are activating, before
    /// its costs are paid, and returns the view of their priority again, as it was before they
    /// began. The choice log forgets the cast, so a replay never sees it. Like a client's
    /// cancel button, this follows the rule for a cast that cannot be completed
    /// ([CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012)): "the casting of the
    /// spell is illegal; the game returns to the moment before the casting of that spell was
    /// proposed".
    pub fn cancel(&mut self) -> Result<PlayerView, SessionError> {
        if self.open().is_none() {
            return Err(SessionError::NotDeciding);
        }
        if !self.cancellable() {
            return Err(SessionError::NotCancellable);
        }
        let before = self
            .before_cast
            .take()
            .expect("a cancellable cast began somewhere");
        self.board.restore(&before.state);
        self.answered = before.answered;
        self.replay.log.truncate(before.log);
        self.undo
            .truncate(before.undo.saturating_sub(self.undo_dropped));
        // Stops changed during the cast stay changed, recorded again at the priority.
        if self.stops != before.stops {
            self.change_stops(self.stops);
        }
        // The priority asked again is a new decision: an answer sent to the cast's is stale.
        let At::Human(_, open) = before.at else {
            unreachable!("a cast begins at the human's decision");
        };
        self.seq += 1;
        self.at = At::Human(self.decision_id(), open);
        self.board.drain_events();
        // The view says nothing happened: the cast never was.
        self.events.clear();
        Ok(self.draw())
    }

    /// Whether the human, at their open decision, may take back an answer ([`Game::undo`]).
    pub(super) fn undoable(&self) -> bool {
        matches!(self.at, At::Human(..)) && !self.undo.is_empty()
    }

    /// Takes back the human's latest answer, at their open decision, and returns the view of
    /// the decision or halt it answered, asked again. A follow-up sent by the client as part
    /// of that same choice (such as a cast's payment plan) goes back with it.
    /// Each call takes back one more, as far back as the [`UNDO_POINTS`] kept, across the
    /// agent's decisions and turns: the agent goes back with the game
    /// ([`Agent::rewind`]) and decides again as play goes on, from the seed it had, so the
    /// same position gets the same search. This is no rule of the game but a practice tool
    /// the human asked for, and it shows them what they would not know: what the agent did,
    /// and the cards drawn since, which come out again in the same order. The choice log keeps
    /// the answers and the agent's choices taken back, then an [`Entry::Undo`], so a replay
    /// goes back the same way.
    pub fn undo(&mut self) -> Result<PlayerView, SessionError> {
        if self.open().is_none() {
            return Err(SessionError::NotDeciding);
        }
        if !self.undoable() {
            return Err(SessionError::NothingToUndo);
        }
        let mut point = self.undo.pop_back().expect("an undoable game has a point");
        while point.continuation {
            let Some(previous) = self.undo.pop_back() else {
                break;
            };
            point = previous;
        }
        self.board.restore(&point.state);
        self.answered = point.answered;
        self.seen = point.seen;
        self.before_cast = point.before_cast;
        self.hurt_by = point.hurt_by;
        self.passing = None;
        if let Some(to) = point.agent
            && self.agent_next() != Some(to)
        {
            match &self.board {
                Boards::Small(_) => Agent::<Small>::rewind(&mut *self.agent, to),
                Boards::Large(_) => Agent::<Large>::rewind(&mut *self.agent, to),
            }
        }
        self.replay.log.push(Entry::Undo);
        // Stops changed since stay changed. The log has them already, and a replay goes back
        // with the same ones.
        self.board
            .set_profile(with_stops(self.board.profile(), self.stops, self.human));
        // What the view there reported of a paused resolution, as it was reported then.
        self.paused = on_board!(&mut self.board, b => {
            paused_events(&b.state, &self.table, &mut b.scratch)
                .iter()
                .map(Relayout::relayout)
                .collect()
        });
        let At::Human(_, open) = point.at else {
            unreachable!("an undo point is at the human's decision");
        };
        // The decision asked again is a new one: an answer sent to an earlier one is stale.
        self.seq += 1;
        self.at = At::Human(self.decision_id(), open);
        self.board.drain_events();
        self.events.clear();
        // The table goes back with the game: each seat's next script starts over.
        self.screens
            .iter_mut()
            .for_each(crate::script::Screen::forget);
        let mut view = self.draw();
        view.undone = true;
        Ok(view)
    }

    /// Keeps the position at the human's open decision, before their answer is applied, as one
    /// they may go back to, dropping the oldest beyond [`UNDO_POINTS`].
    pub(super) fn keep_undo_point(&mut self, continuation: bool) {
        self.undo.push_back(UndoPoint {
            state: self.board.snapshot(),
            continuation,
            at: self.at.clone(),
            seen: self.seen,
            answered: self.answered.clone(),
            before_cast: self.before_cast.clone(),
            agent: self.agent_next(),
            hurt_by: self.hurt_by,
        });
        if self.undo.len() > UNDO_POINTS {
            self.undo.pop_front();
            self.undo_dropped += 1;
        }
    }

    /// How many undo points stand before the human's open decision, counting those dropped
    /// beyond [`UNDO_POINTS`]: the decision's place among their answers, the same whenever an
    /// undo comes back to it ([`crate::protocol::DecisionView::undo_depth`]).
    pub(super) fn undo_depth(&self) -> usize {
        self.undo_dropped + self.undo.len()
    }
}
