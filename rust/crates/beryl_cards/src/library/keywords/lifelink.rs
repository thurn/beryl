//! Lifelink ([CR 702.15](https://yawgatog.com/resources/magic-rules/#R70215)), on a card
//! complete from the card store.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// A 2/1 with lifelink, complete from the store.
pub const LIFELINKER: &str = names::CHILD_OF_NIGHT;

/// Damage the lifelinker deals gains its controller that much life
/// ([CR 702.15b](https://yawgatog.com/resources/magic-rules/#R70215b)): "Damage dealt by a
/// source with lifelink causes that source's controller, or its owner if it has no controller,
/// to gain that much life (in addition to any other results that damage causes)." That is
/// damage to a player and damage to a creature alike.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("A lifelinker's damage to a player gains its controller that much life")
        .permanents(P0, &[Permanent::new(LIFELINKER)])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, LIFELINKER),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::EndOfCombat,
            },
            Action::assert(Assertion::Life {
                player: P0,
                life: 22,
            }),
            Action::assert(Assertion::Life {
                player: P1,
                life: 18,
            }),
        ]),
    Scenario::new("A lifelinker's damage to a blocker gains its controller that much life")
        .permanents(P0, &[Permanent::new(LIFELINKER)])
        .permanents(P1, &[Permanent::new(props::SMALL_CREATURE)])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, LIFELINKER),
            Action::choose(P1, DecisionKind::Blocker, props::SMALL_CREATURE),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::EndOfCombat,
            },
            Action::assert(Assertion::Life {
                player: P0,
                life: 22,
            }),
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
