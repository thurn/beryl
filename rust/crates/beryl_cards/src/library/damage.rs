//! Damage ([CR 120](https://yawgatog.com/resources/magic-rules/#R120)).

use beryl_engine::TurnStep;
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

/// Damage an effect deals to a player is loss of life for that player alone
/// ([CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a)): "Damage dealt to a
/// player by a source without infect causes that player to lose that much life." The seed is
/// one whose first flip is lost, so the artifact deals 3 to its controller.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Damage an effect deals to a player is that player's life lost")
            .seed(LOST_SEED)
            .at(1, P0, TurnStep::PostcombatMain)
            .permanents(P1, &[Permanent::new(names::MANA_CRYPT)])
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
                    life: 17,
                }),
                Action::assert(Assertion::Life {
                    player: P0,
                    life: 20,
                }),
            ]),
    ];

const LOST_SEED: u64 = 2;
