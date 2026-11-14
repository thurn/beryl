//! The human's pass on to their next stop, while the session passes their priority for them.

use beryl_engine::{DecisionKind, NextStop, Observation, TurnStep, view};

use crate::replay::Entry;

use super::{At, Boards, Game, Open, on_board};

/// The human's pass on to their next stop ([`Game::pass_on`]), while the session passes their
/// priority for them.
#[derive(Clone, Copy, Debug)]
pub(super) struct PassingOn {
    /// The turn and step of their next stop, where it ends.
    pub(super) until: (u16, TurnStep),
}

impl Game {
    /// Begins passing on to the human's next stop from their open decision, if it can pass on,
    /// and records it. Returns whether it began.
    pub(super) fn start_passing(&mut self) -> bool {
        let Some(until) = self.pass_on_target() else {
            return false;
        };
        self.passing = Some(PassingOn { until });
        self.replay.log.push(Entry::PassOn);
        true
    }

    /// The turn and step of the human's next stop in a later step, if their open decision is a
    /// priority from which passing would stop them again in this one
    /// ([`crate::protocol::DecisionView::pass_on`]).
    pub(super) fn pass_on_target(&mut self) -> Option<(u16, TurnStep)> {
        if self.reviewing()
            || !matches!(&self.at, At::Human(_, Open::Decision(d)) if d.kind == DecisionKind::Priority)
        {
            return None;
        }
        let next = on_board!(&mut self.board, b => {
            let obs = Observation::new(view(&b.state, &self.table, &mut b.scratch), self.human);
            crate::protocol::pass_on(obs, self.stops)
        })?;
        let turn = self.board.turn().number();
        Some(match next {
            NextStop::Step(step) => (turn, step),
            NextStop::NextTurn => (turn + 1, TurnStep::Untap),
            NextStop::Resolve | NextStop::CombatDamage => {
                unreachable!("a later step's stop is a step or the next turn")
            }
        })
    }

    /// Whether the session passes the human's priority for them as they pass on to their next
    /// stop ([`Game::pass_on`]): whenever it is short of that stop, through the opponent's
    /// spells, abilities and attacks as well as the human's own (UI Conventions, "Passing to
    /// the end step"). The human asked for the end step, and a pass button that stopped at
    /// each opponent spell would do no more than "Resolve" beside it.
    pub(super) fn passes_for(&self, passing: PassingOn) -> bool {
        let turn = self.board.turn();
        (turn.number(), turn.step()) < passing.until
    }
}
