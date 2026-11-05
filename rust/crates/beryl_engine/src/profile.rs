//! Restriction profiles: which decisions are offered, and the budgets (architecture §12.3,
//! §15).

use crate::state::{State, Turn, TurnStep};

/// Where players get priority as a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stops {
    /// Every priority with a legal action is a decision (§7.2).
    Exact,
    /// Priority only at the smart stops (§15, decision D18); elsewhere both players pass
    /// silently.
    Smart,
}

/// The budgets whose exhaustion is `Outcome::Stalled` (architecture §12.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    pub decisions_per_turn: u16,
    pub turns: u16,
    pub state_based_iterations: u8,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        decisions_per_turn: 500,
        turns: 200,
        state_based_iterations: 32,
    };
}

/// A turn and step where the active player's first priority is a decision even when passing
/// is all they can do, so a caller can stop the game where the engine would otherwise pass
/// silently. The scenario runner uses one to advance to a step (architecture §17.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Breakpoint {
    pub turn: u16,
    pub step: TurnStep,
}

/// A restriction of the decision surface plus budgets. Every profile is sound: any line it
/// produces is a legal line of the real game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Profile {
    pub stops: Stops,
    pub limits: Limits,
    pub breakpoint: Option<Breakpoint>,
}

impl Profile {
    /// Everything the rules offer, less what §7.2 settles silently.
    pub const EXACT: Profile = Profile {
        stops: Stops::Exact,
        limits: Limits::DEFAULT,
        breakpoint: None,
    };
    /// The smart stops, for search and, by default, for humans.
    pub const SMART: Profile = Profile {
        stops: Stops::Smart,
        limits: Limits::DEFAULT,
        breakpoint: None,
    };

    /// Whether `player`'s priority now is a decision, given they have a legal action.
    pub(crate) fn stops_for(self, s: &State, player: u8) -> bool {
        match self.stops {
            Stops::Exact => true,
            Stops::Smart => is_smart_stop(s, player),
        }
    }

    /// Whether the open priority is the active player's first in the breakpoint's step.
    pub(crate) fn breaks_at(self, turn: Turn) -> bool {
        self.breakpoint.is_some_and(|b| {
            b.turn == turn.number
                && b.step == turn.step
                && turn.priority == turn.active
                && turn.passes == 0
        })
    }
}

/// The smart stops (§15): "their own main phases, after attackers and after blockers are
/// declared, the opponent's end step, and whenever the stack is non-empty."
fn is_smart_stop(s: &State, player: u8) -> bool {
    if !s.stack.is_empty() {
        return true;
    }
    let active = s.turn.active == player;
    match s.turn.step {
        TurnStep::PrecombatMain | TurnStep::PostcombatMain => active,
        TurnStep::DeclareAttackers | TurnStep::DeclareBlockers => crate::turn::any_attacking(s),
        TurnStep::End => !active,
        _ => false,
    }
}
