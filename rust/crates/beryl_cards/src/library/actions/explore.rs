//! Explore ([CR 701.44](https://yawgatog.com/resources/magic-rules/#R70144)).

use beryl_engine::state::CounterKind;
use beryl_engine::{TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::tokens::MAP_TOKEN;
use crate::{names, props};

/// A land card revealed goes to its controller's hand, a nonbasic one as a basic
/// ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): "If a land card is
/// revealed this way, that player puts that card into their hand." The exploring creature
/// then gets no counter, and nothing is asked.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A creature that explores a nonbasic land gets no counter")
            .at(2, P1, TurnStep::PrecombatMain)
            .permanents(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[names::GET_LOST])
            .library(P0, &[names::PLAINS])
            .permanents(
                P1,
                &[
                    Permanent::new(props::CREATURE),
                    Permanent::new(props::CREATURE),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P1, &[names::ISLAND])
            .library(P1, &[props::NONBASIC_LAND, names::ISLAND])
            .actions(&[
                Action::pass(P1),
                Action::cast(P0, names::GET_LOST),
                Action::activate(P1, MAP_TOKEN, 0),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(props::NONBASIC_LAND),
                    count: 1,
                }),
                Action::assert(Assertion::Counters {
                    player: P1,
                    card: props::CREATURE,
                    kind: CounterKind::PlusOne,
                    count: 0,
                }),
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[names::ISLAND],
                }),
            ]),
    ];
