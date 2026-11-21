//! Zones ([CR 400](https://yawgatog.com/resources/magic-rules/#R400)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::names;
use crate::tokens::TREASURE_TOKEN;

const CONVERTER: &str = names::CURRENCY_CONVERTER;

/// A card put into a graveyard from exile goes to its owner's
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." What the effect then says of "it" is said of that card: a land.
pub const PUT_INTO_GRAVEYARD: &[Scenario] =
    &[
        Scenario::new("A land card put into the graveyard from exile is still the card it was")
            .permanents(
                P0,
                &[
                    Permanent::new(CONVERTER),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[names::PLAINS])
            .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::activate(P0, CONVERTER, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::PLAINS),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, CONVERTER, 2),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(names::PLAINS),
                    count: 1,
                }),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: TREASURE_TOKEN,
                    count: 1,
                }),
            ]),
    ];

/// A hand is a hidden zone
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Hidden zones are zones
/// in which not all players can be expected to see the cards' faces." A player told to look
/// at an empty one sees nothing, and the rest of the effect finds no card
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)).
pub const LOOK_AT: &[Scenario] = &[Scenario::new("Looking at an empty hand finds no card")
    .hand(P0, &[names::ELITE_SPELLBINDER, names::ISLAND])
    .permanents(P0, &[Permanent::new(names::PLAINS); 3])
    .library(P0, &[names::PLAINS])
    .library(P1, &[names::ISLAND])
    .actions(&[
        Action::cast(P0, names::ELITE_SPELLBINDER),
        Action::Resolve,
        Action::Resolve,
        Action::assert(Assertion::Stack { count: 0 }),
        Action::assert(Assertion::Count {
            player: P1,
            zone: Zone::Exile,
            card: None,
            count: 0,
        }),
    ])];

/// A graveyard ability can trigger and return its own card without a source on the
/// battlefield.
pub const RETURN_FROM_GRAVEYARD: &[Scenario] =
    &[
        Scenario::new("a land entering returns it from the graveyard")
            .graveyard(P0, &[names::BLOODGHAST])
            .hand(P0, &[names::SWAMP])
            .actions(&[
                ME.plays(names::SWAMP),
                Resolve,
                ME.yes(),
                ME.controls(names::BLOODGHAST, 1),
                ME.has(Zone::Graveyard, names::BLOODGHAST, 0),
            ]),
    ];

/// Declining to pay life modifies how the land enters the battlefield.
/// [CR 614.12] "Some replacement effects modify how a permanent enters the battlefield."
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
pub const DECLINE_LIFE_TO_ENTER_UNTAPPED: &[Scenario] =
    &[
        Scenario::new("Soporific Springs: no life paid, it enters tapped")
            .hand(P0, &[names::SINK_INTO_STUPOR])
            .library(P1, &[names::PLAINS])
            .actions(&[
                ME.plays(names::SINK_INTO_STUPOR),
                ME.no(),
                ME.life(20),
                ME.tapped("Soporific Springs", 1),
            ]),
    ];

/// Returning from a graveyard needs the same object that triggered. A land controlled by
/// the other player does not trigger the ability.
pub const GRAVEYARD_RETURN: &[Scenario] = &[
    Scenario::new("a land entering does not trigger Bloodghast on the battlefield")
        .battlefield(P0, &[names::BLOODGHAST])
        .hand(P0, &[names::SWAMP])
        .actions(&[
            ME.plays(names::SWAMP),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.done_attacking(),
            stack(0),
            advance_to(1, TurnStep::PostcombatMain),
            ME.controls(names::BLOODGHAST, 1),
        ]),
    Scenario::new("only its owner's land triggers Bloodghast in a graveyard")
        .at(2, P1, TurnStep::PrecombatMain)
        .graveyard(P0, &[names::BLOODGHAST])
        .hand(P0, &[names::SWAMP])
        .hand(P1, &[names::PLAINS])
        .library(P0, &[names::ISLAND])
        .actions(&[
            THEM.plays(names::PLAINS),
            advance_to(2, TurnStep::PostcombatMain),
            ME.has(Zone::Graveyard, names::BLOODGHAST, 1),
            advance_to(3, TurnStep::PrecombatMain),
            ME.plays(names::SWAMP),
            Resolve,
            ME.yes(),
            advance_to(3, TurnStep::PostcombatMain),
            ME.controls(names::BLOODGHAST, 1),
            ME.tapped(names::BLOODGHAST, 0),
        ]),
    Scenario::new("Bloodghast stays in exile when its graveyard trigger resolves")
        .graveyard(P0, &[names::BLOODGHAST])
        .hand(P0, &[names::SWAMP])
        .battlefield(P1, &[names::TORMODS_CRYPT])
        .actions(&[
            ME.plays(names::SWAMP),
            THEM.activates(names::TORMODS_CRYPT, 0),
            THEM.targets_player(ME),
            Resolve,
            Resolve,
            ME.yes(),
            advance_to(1, TurnStep::PostcombatMain),
            ME.has(Zone::Exile, names::BLOODGHAST, 1),
            ME.controls(names::BLOODGHAST, 0),
        ]),
];
