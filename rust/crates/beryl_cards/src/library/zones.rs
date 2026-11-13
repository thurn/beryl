//! Zones ([CR 400](https://yawgatog.com/resources/magic-rules/#R400)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

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
