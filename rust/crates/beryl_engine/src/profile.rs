//! Restriction profiles: which decisions are offered, and the budgets (architecture §12.3,
//! §15).

use crate::state::{State, TurnStep};

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

/// A restriction of the decision surface plus budgets. Every profile is sound: any line it
/// produces is a legal line of the real game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Profile {
    pub stops: Stops,
    pub limits: Limits,
}

impl Profile {
    /// Everything the rules offer, less what §7.2 settles silently.
    pub const EXACT: Profile = Profile {
        stops: Stops::Exact,
        limits: Limits::DEFAULT,
    };
    /// The smart stops, for search and, by default, for humans.
    pub const SMART: Profile = Profile {
        stops: Stops::Smart,
        limits: Limits::DEFAULT,
    };

    /// Whether `player`'s priority now is a decision, given they have a legal action.
    pub(crate) fn stops_for(self, s: &State, player: u8) -> bool {
        match self.stops {
            Stops::Exact => true,
            Stops::Smart => is_smart_stop(s, player),
        }
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
