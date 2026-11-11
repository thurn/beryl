//! The boards the verb suite watches (`ui/verbs.json`, `ui/scripts/verbs.mjs`): each plays the
//! changes a verb's check asks for, recorded from both seats.

use beryl_cards::names::{
    AANGS_ICEBERG, COUNTERSPELL, DAMN, ISLAND, MANA_CRYPT, OVERLORD_OF_THE_MISTMOORS, PLAINS,
    PORTABLE_HOLE, REPRIEVE, SNAPCASTER_MAGE, STOCK_UP, THIRST_FOR_DISCOVERY, TRINKET_MAGE,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{INSECT_TOKEN, ISLAND_, LANDS, PLAINS_, cast_with_option, choose};

/// Boards for the verb suite.
pub(super) const VERBS: &[Scenario] = &[
    // Cards changing zone, every way the first seat's decks can show: it draws for its turn;
    // casts Thirst for Discovery, which the opponent answers with Snapcaster Mage, which it
    // counters; draws three and discards a land; destroys the opponent's creature and token
    // with an overloaded Damn; exiles their Mana Crypt under Portable Hole; and in the
    // opponent's turn returns their Stock Up to their hand with Reprieve, drawing a card.
    Scenario::new("verbs-zones")
        .at(3, P0, TurnStep::Upkeep)
        .battlefield(
            P0,
            &[
                ISLAND_, ISLAND_, ISLAND_, ISLAND_, ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_,
                PLAINS_, PLAINS_, PLAINS_, PLAINS_,
            ],
        )
        .hand(
            P0,
            &[
                THIRST_FOR_DISCOVERY,
                COUNTERSPELL,
                DAMN,
                PORTABLE_HOLE,
                REPRIEVE,
            ],
        )
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(TRINKET_MAGE),
                Permanent::token(INSECT_TOKEN),
                Permanent::new(MANA_CRYPT),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
            ],
        )
        .hand(P1, &[SNAPCASTER_MAGE, STOCK_UP])
        // The Insect token's maker.
        .graveyard(P1, &[OVERLORD_OF_THE_MISTMOORS])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            Action::cast(P1, SNAPCASTER_MAGE),
            Action::cast(P0, COUNTERSPELL),
            choose(P0, DecisionKind::Target, SNAPCASTER_MAGE),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            choose(P0, DecisionKind::ChooseObject, ISLAND),
            cast_with_option(P0, DAMN),
            Action::cast(P0, PORTABLE_HOLE),
            choose(P0, DecisionKind::Target, MANA_CRYPT),
            Action::cast(P1, STOCK_UP),
            Action::cast(P0, REPRIEVE),
            choose(P0, DecisionKind::Target, STOCK_UP),
        ]),
    // The first seat scries two with Aang's Iceberg and puts one of them on the bottom.
    Scenario::new("verbs-scry")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[Permanent::new(AANGS_ICEBERG), PLAINS_, PLAINS_, ISLAND_],
        )
        .hand(P0, &[COUNTERSPELL])
        .library(P0, &[COUNTERSPELL, ISLAND, PLAINS, ISLAND])
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P0, AANGS_ICEBERG, 1),
            choose(P0, DecisionKind::ChooseUpTo, ISLAND),
            Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
        ]),
];
