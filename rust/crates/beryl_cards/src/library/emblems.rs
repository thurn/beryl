//! Emblems ([CR 114](https://yawgatog.com/resources/magic-rules/#R114)).

use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

const TEFERI: &str = names::TEFERI_HERO_OF_DOMINARIA;

/// An emblem is its player's, not its source's
/// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "The emblem is both owned
/// and controlled by that player." The planeswalker that made it is put into its owner's
/// graveyard with no loyalty left, and the emblem's ability triggers on that player's next
/// draw all the same.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("An emblem's ability triggers after the permanent that made it is gone")
            .permanents(
                P0,
                &[Permanent::new(TEFERI).counters(CounterKind::Loyalty, 8)],
            )
            .library(P0, &[names::ISLAND, names::ISLAND])
            .permanents(
                P1,
                &[
                    Permanent::new(props::ARTIFACT),
                    Permanent::new(names::ISLAND),
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, TEFERI, 2),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(TEFERI),
                    count: 1,
                }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::Upkeep,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Target, props::ARTIFACT),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
            ]),
    ];
