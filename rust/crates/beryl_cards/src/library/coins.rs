//! Flipping a coin ([CR 705](https://yawgatog.com/resources/magic-rules/#R705)).

use beryl_engine::TurnStep;
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

/// A flip won ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "If the call
/// matches the result, the player wins the flip." What the effect does on a loss does not
/// happen, to either player. The seed is one whose first flip is won.
pub const SCENARIOS: &[Scenario] = &[Scenario::new("A flip won does nothing a loss would")
    .seed(WON_SEED)
    .at(1, P0, TurnStep::PostcombatMain)
    .battlefield(P1, &[Permanent::new(names::MANA_CRYPT)])
    .library(P0, &[names::ISLAND])
    .library(P1, &[names::ISLAND, names::ISLAND])
    .actions(&[
        Action::AdvanceTo {
            turn: 2,
            step: TurnStep::Upkeep,
        },
        Action::pass(P1),
        Action::assert(Assertion::Life {
            player: P1,
            life: 20,
        }),
        Action::assert(Assertion::Life {
            player: P0,
            life: 20,
        }),
    ])];

const WON_SEED: u64 = 0;
