//! Landwalk ([CR 702.14](https://yawgatog.com/resources/magic-rules/#R70214)), on a card complete
//! from the card store.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// A 3/3 with swampwalk, complete from the store.
pub const LANDWALKER: &str = names::BOG_WRAITH;

/// The swampwalker can't be blocked while the defending player controls a Swamp
/// ([CR 702.14c](https://yawgatog.com/resources/magic-rules/#R70214c)): "A creature with
/// landwalk can't be blocked as long as the defending player controls at least one land with
/// the specified land type (as in “islandwalk”)". Against an Island it is blocked like any
/// creature.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("A swampwalker can't be blocked by a player who controls a Swamp")
        .permanents(P0, &[Permanent::new(LANDWALKER)])
        .permanents(
            P1,
            &[
                Permanent::new(names::SWAMP),
                Permanent::new(props::SMALL_CREATURE),
            ],
        )
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, LANDWALKER),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::EndOfCombat,
            },
            Action::assert(Assertion::Life {
                player: P1,
                life: 17,
            }),
        ]),
    Scenario::new("A swampwalker is blocked by a player who controls no Swamp")
        .permanents(
            P0,
            &[Permanent::new(LANDWALKER), Permanent::new(names::SWAMP)],
        )
        .permanents(
            P1,
            &[
                Permanent::new(names::ISLAND),
                Permanent::new(props::SMALL_CREATURE),
            ],
        )
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, LANDWALKER),
            Action::choose(P1, DecisionKind::Blocker, props::SMALL_CREATURE),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::EndOfCombat,
            },
            Action::assert(Assertion::Life {
                player: P1,
                life: 20,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(props::SMALL_CREATURE),
                count: 1,
            }),
        ]),
];
