//! Counter ([CR 701.6](https://yawgatog.com/resources/magic-rules/#R7016)).

use beryl_engine::Zone;
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// A countered spell put somewhere other than its owner's graveyard
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "later text on the card
/// may modify the meaning of earlier text". The creature spell takes both of its
/// controller's lands, so nothing pays {3} and it is countered into exile, never a
/// permanent and never in a graveyard.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A spell countered into exile is in no graveyard")
            .battlefield(
                P0,
                &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
            )
            .hand(P0, &[props::SMALL_CREATURE])
            .battlefield(
                P1,
                &[Permanent::new(names::PLAINS), Permanent::new(names::ISLAND)],
            )
            .hand(P1, &[names::NO_MORE_LIES])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::SMALL_CREATURE),
                Action::cast(P1, names::NO_MORE_LIES),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: Some(props::SMALL_CREATURE),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 0,
                }),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::SMALL_CREATURE,
                    count: 0,
                }),
            ]),
    ];
