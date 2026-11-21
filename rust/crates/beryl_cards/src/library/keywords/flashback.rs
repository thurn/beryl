//! Flashback ([CR 702.34](https://yawgatog.com/resources/magic-rules/#R70234)).

use beryl_engine::{TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM};

use crate::{names, props};

/// A sorcery card that gains flashback is cast from its owner's graveyard for its mana cost,
/// and exiled as it resolves
/// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "If the flashback
/// cost was paid, exile this card instead of putting it anywhere else any time it would
/// leave the stack."
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A sorcery that gains flashback is cast from the graveyard and exiled")
            .permanents(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[names::SNAPCASTER_MAGE])
            .graveyard(P0, &[props::SORCERY.card])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS, names::PLAINS])
            .actions(&[
                Action::cast(P0, names::SNAPCASTER_MAGE),
                // Snapcaster Mage, then its trigger, whose one target is the sorcery.
                Action::Resolve,
                Action::Resolve,
                // Its one target is that creature, whose controller gains 3 life.
                Action::cast(P0, props::SORCERY.card),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: Some(props::SORCERY.card),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 0,
                }),
                Action::assert(Assertion::Life {
                    player: P0,
                    life: 23,
                }),
            ]),
    ];

/// An instant granted flashback can respond to another spell. The session tests check
/// its movement from the graveyard to the stack.
pub const INSTANT_RESPONSE: &[Scenario] =
    &[
        Scenario::new("Snapcaster Mage: flashback a counterspell, then exile it")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[names::ISLAND; 4])
            .hand(P0, &[names::SNAPCASTER_MAGE])
            .graveyard(P0, &[names::COUNTERSPELL])
            .battlefield(P1, &[names::ISLAND; 3])
            .hand(P1, &[names::TIMETWISTER, names::PLAINS])
            .library(P1, &[names::PLAINS])
            .actions(&[
                THEM.casts(names::TIMETWISTER),
                ME.casts(names::SNAPCASTER_MAGE),
                // Snapcaster, then its trigger, which no one can respond to.
                Resolve,
                THEM.passes(),
                // Counterspell resolves at once: neither player can respond.
                ME.casts(names::COUNTERSPELL),
                THEM.has(Zone::Graveyard, names::TIMETWISTER, 1),
                ME.has(Zone::Exile, names::COUNTERSPELL, 1),
                ME.has(Zone::Graveyard, names::COUNTERSPELL, 0),
            ]),
    ];
