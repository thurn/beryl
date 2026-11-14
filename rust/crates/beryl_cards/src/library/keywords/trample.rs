//! Trample ([CR 702.19](https://yawgatog.com/resources/magic-rules/#R70219)), on a card complete
//! from the card store.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// A 6/6 with trample, complete from the store.
pub const TRAMPLER: &str = names::COLOSSAL_DREADMAW;

/// Blocked by a 2/1, the trampler assigns lethal damage to it and the rest to the player
/// ([CR 702.19b](https://yawgatog.com/resources/magic-rules/#R70219b)): "Once all those
/// blocking creatures are assigned lethal damage, any excess damage is assigned as its
/// controller chooses among those blocking creatures and the player, planeswalker, or battle
/// the creature is attacking." So 1 goes to the blocker and 5 to the player.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A trampler assigns lethal damage to its blocker and the rest to the player")
            .permanents(P0, &[Permanent::new(TRAMPLER)])
            .permanents(P1, &[Permanent::new(props::SMALL_CREATURE)])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, TRAMPLER),
                Action::choose(P1, DecisionKind::Blocker, props::SMALL_CREATURE),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::EndOfCombat,
                },
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 15,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(props::SMALL_CREATURE),
                    count: 1,
                }),
            ]),
    ];
