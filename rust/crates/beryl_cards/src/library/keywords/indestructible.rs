//! Indestructible ([CR 702.12](https://yawgatog.com/resources/magic-rules/#R70212)), on a card
//! complete from the card store.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// A 0/1 artifact creature with indestructible, complete from the store.
pub const INDESTRUCTIBLE: &str = names::DARKSTEEL_MYR;

/// The indestructible creature survives lethal damage and an effect that destroys it
/// ([CR 702.12b](https://yawgatog.com/resources/magic-rules/#R70212b)): "A permanent with
/// indestructible can't be destroyed. Such permanents aren't destroyed by lethal damage, and
/// they ignore the state-based action that checks for lethal damage (see rule 704.5g)."
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("An indestructible blocker survives lethal combat damage")
        .permanents(P0, &[Permanent::new(props::SMALL_CREATURE)])
        .permanents(P1, &[Permanent::new(INDESTRUCTIBLE)])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, props::SMALL_CREATURE),
            Action::choose(P1, DecisionKind::Blocker, INDESTRUCTIBLE),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::EndOfCombat,
            },
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Battlefield,
                card: Some(INDESTRUCTIBLE),
                count: 1,
            }),
            Action::assert(Assertion::Life {
                player: P1,
                life: 20,
            }),
        ]),
    Scenario::new("An indestructible creature is not destroyed by an effect")
        .interaction(&[names::GET_LOST])
        .permanents(
            P0,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
            ],
        )
        .hand(P0, &[names::GET_LOST])
        .permanents(P1, &[Permanent::new(INDESTRUCTIBLE)])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::GET_LOST),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Battlefield,
                card: Some(INDESTRUCTIBLE),
                count: 1,
            }),
        ]),
];
