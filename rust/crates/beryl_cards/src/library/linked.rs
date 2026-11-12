//! Linked abilities ([CR 607](https://yawgatog.com/resources/magic-rules/#R607)).

use beryl_engine::Zone;
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::tokens::ROGUE_TOKEN;
use crate::{names, props};

const CONVERTER: &str = names::CURRENCY_CONVERTER;

/// A card another object's effect exiled is not among the cards exiled with this one
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
/// refers only to cards in the exile zone that were put there as a result of an instruction
/// to exile them in the first ability." So the ability that puts one of them into a
/// graveyard finds none.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A card exiled by another permanent is not exiled with this one")
            .battlefield(
                P0,
                &[Permanent::new(CONVERTER), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[props::ARTIFACT, names::ISLAND])
            .library(P0, &[names::PLAINS])
            .battlefield(P1, &[Permanent::new(props::SMALL_CREATURE)])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                // The artifact, then its trigger, whose one target is the creature.
                Action::Resolve,
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(props::SMALL_CREATURE),
                    count: 1,
                }),
                Action::activate(P0, CONVERTER, 2),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(props::SMALL_CREATURE),
                    count: 1,
                }),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: ROGUE_TOKEN,
                    count: 0,
                }),
            ]),
    ];
