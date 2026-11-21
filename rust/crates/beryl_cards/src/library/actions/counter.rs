//! Counter ([CR 701.6](https://yawgatog.com/resources/magic-rules/#R7016)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::{names, props};

/// A countered spell put somewhere other than its owner's graveyard
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "later text on the card
/// may modify the meaning of earlier text". The creature spell takes both of its
/// controller's lands, so nothing pays {3} and it is countered into exile, never a
/// permanent and never in a graveyard.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A spell countered into exile is in no graveyard")
            .permanents(
                P0,
                &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
            )
            .hand(P0, &[props::VANILLA.card])
            .permanents(
                P1,
                &[Permanent::new(names::PLAINS), Permanent::new(names::ISLAND)],
            )
            .hand(P1, &[names::NO_MORE_LIES])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::VANILLA.card),
                Action::cast(P1, names::NO_MORE_LIES),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: Some(props::VANILLA.card),
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
                    card: props::VANILLA.card,
                    count: 0,
                }),
            ]),
    ];

/// The default destination, exercised by the scenario and the session's animation checks.
pub const GRAVEYARD: &[Scenario] =
    &[
        Scenario::new("Counterspell counters a spell into its owner's graveyard")
            .battlefield(P0, &[names::ISLAND, names::ISLAND])
            .hand(P0, &[names::COUNTERSPELL])
            .battlefield(P1, &[names::PLAINS])
            .hand(P1, &[props::ARTIFACT.card, names::PLAINS])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(props::ARTIFACT.card),
                ME.casts(names::COUNTERSPELL),
                THEM.has(Zone::Graveyard, props::ARTIFACT.card, 1),
                THEM.has(Zone::Battlefield, props::ARTIFACT.card, 0),
                ME.has(Zone::Graveyard, names::COUNTERSPELL, 1),
            ]),
    ];

/// Mana Tithe offers payment when the targeted spell's controller can afford it.
/// The review page also uses the unaffordable case to let both players try their casts.
pub const UNLESS_PAID: &[Scenario] = &[
    Scenario::new("Mana Tithe: the controller pays {1}")
        .battlefield(P0, &[names::PLAINS, names::PLAINS])
        .hand(P0, &[props::ARTIFACT.card])
        .battlefield(P1, &[names::PLAINS])
        .hand(P1, &[names::MANA_TITHE])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            THEM.casts(names::MANA_TITHE),
            ME.yes(),
            ME.has(Zone::Battlefield, props::ARTIFACT.card, 1),
            ME.tapped(names::PLAINS, 2),
            THEM.has(Zone::Graveyard, names::MANA_TITHE, 1),
        ]),
    Scenario::new("Mana Tithe: lands left after paying for the spell pay {1}")
        .battlefield(
            P0,
            &[
                names::ISLAND,
                props::NONBASIC_LAND.card,
                names::PLAINS,
                names::PLAINS,
                names::ISLAND,
                names::PLAINS,
            ],
        )
        .hand(P0, &[props::PLANESWALKER.card])
        .battlefield(P1, &[names::PLAINS])
        .hand(P1, &[names::MANA_TITHE])
        .actions(&[
            ME.casts(props::PLANESWALKER.card),
            THEM.casts(names::MANA_TITHE),
            ME.yes(),
            advance_to(1, TurnStep::End),
            ME.controls(props::PLANESWALKER.card, 1),
            THEM.has(Zone::Graveyard, names::MANA_TITHE, 1),
        ]),
    Scenario::new("Mana Tithe: a controller who can't pay isn't asked")
        .battlefield(P0, &[names::PLAINS])
        .hand(P0, &[props::ARTIFACT.card])
        .battlefield(P1, &[names::PLAINS])
        .hand(P1, &[names::MANA_TITHE])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            THEM.casts(names::MANA_TITHE),
            ME.has(Zone::Graveyard, props::ARTIFACT.card, 1),
            ME.has(Zone::Battlefield, props::ARTIFACT.card, 0),
        ]),
];

pub const SELF_TARGET: &[Scenario] =
    &[
        Scenario::new("A counterspell is not a legal target for itself")
            .hand(P0, &[names::COUNTERSPELL, names::ISLAND])
            .battlefield(P0, &[names::ISLAND; 2])
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
    ];

pub const SAME_NAME: &[Scenario] =
    &[
        Scenario::new("A counterspell offers two same-named spells with different targets")
            .interaction(&[names::LIGHTNING_BOLT])
            .battlefield(P0, &[names::ISLAND; 2])
            .hand(P0, &[names::COUNTERSPELL])
            .battlefield(P1, &[names::MOUNTAIN; 2])
            .hand(P1, &[names::LIGHTNING_BOLT; 2])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(names::LIGHTNING_BOLT),
                THEM.targets_player(ME),
                THEM.casts(names::LIGHTNING_BOLT),
                THEM.targets_player(THEM),
                ME.casts(names::COUNTERSPELL),
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(names::LIGHTNING_BOLT),
                Resolve,
                Resolve,
                stack(0),
                THEM.has(Zone::Graveyard, names::LIGHTNING_BOLT, 2),
            ]),
    ];
