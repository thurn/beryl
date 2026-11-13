//! Delayed triggered abilities
//! ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).

use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::{names, props};

const TEFERI: &str = names::TEFERI_HERO_OF_DOMINARIA;

/// A delayed ability an effect made triggers once
/// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)): "A delayed triggered
/// ability will trigger only once--the next time its trigger event occurs--unless it has a
/// stated duration". Its controller taps two Plains for two artifacts and untaps one of them
/// at the first end step. At the second nothing triggers: the other Plains is still tapped
/// and the stack is empty.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A delayed ability triggers at the next end step and at no later one")
            .permanents(
                P0,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[props::ARTIFACT, props::ARTIFACT])
            .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
            .permanents(P1, &[Permanent::new(names::ISLAND)])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::Resolve,
                Action::cast(P0, props::ARTIFACT),
                Action::Resolve,
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Stack { count: 1 }),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseUpTo, names::PLAINS),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Stack { count: 0 }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 1,
                }),
            ]),
    ];
