//! The boards the verb suite watches (`ui/verbs.json`, `ui/scripts/verbs.mjs`): each plays the
//! changes a verb's check asks for, recorded from both seats.

use beryl_cards::names::{
    AANGS_ICEBERG, BEZA_THE_BOUNDING_SPRING, COUNTERSPELL, CURRENCY_CONVERTER, DAMN,
    ELITE_SPELLBINDER, ISLAND, MANA_CRYPT, OUST, OVERLORD_OF_THE_MISTMOORS, PLAINS, PORTABLE_HOLE,
    REPRIEVE, SNAPCASTER_MAGE, STOCK_UP, SWIFT_RECONFIGURATION, THE_WANDERING_EMPEROR,
    THIEVING_SKYDIVER, THIRST_FOR_DISCOVERY, TRINKET_MAGE,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{
    INSECT_TOKEN, ISLAND_, LANDS, PLAINS_, TREASURE_TOKEN, cast_with_option, choose, play_land,
};

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
    // Combat damage every way, and what follows it: the first seat attacks with Trinket Mage,
    // which Beza blocks and kills, taking its damage; with Snapcaster Mage, which strikes the
    // player; and with Elite Spellbinder, which strikes The Wandering Emperor. In their turn
    // the opponent taps and sacrifices a Treasure for the Oust that puts Snapcaster Mage into
    // its library, so its controller gains life. In the turn after, the attacker that is left
    // untaps and Mana Crypt flips its coin.
    Scenario::new("verbs-combat")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(TRINKET_MAGE),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(MANA_CRYPT),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(THE_WANDERING_EMPEROR),
                Permanent::token(TREASURE_TOKEN),
                ISLAND_,
            ],
        )
        .hand(P1, &[OUST])
        .library(P1, LANDS)
        .actions(&[
            choose(P0, DecisionKind::Attacker, TRINKET_MAGE),
            Action::answer(P0, DecisionKind::AttackTarget, Pick::DefendingPlayer),
            choose(P0, DecisionKind::Attacker, SNAPCASTER_MAGE),
            Action::answer(P0, DecisionKind::AttackTarget, Pick::DefendingPlayer),
            choose(P0, DecisionKind::Attacker, ELITE_SPELLBINDER),
            choose(P0, DecisionKind::AttackTarget, THE_WANDERING_EMPEROR),
            choose(P1, DecisionKind::Blocker, BEZA_THE_BOUNDING_SPRING),
            choose(P1, DecisionKind::BlockTarget, TRINKET_MAGE),
            Action::cast(P1, OUST),
            choose(P1, DecisionKind::Target, SNAPCASTER_MAGE),
        ]),
    // Changes on a card where it lies: a seventh land piles the first seat's lands; its
    // face-down Snapcaster Mage turns face up, and The Wandering Emperor puts a counter on it;
    // Thieving Skydiver, kicked, takes the opponent's Currency Converter across the table;
    // and Swift Reconfiguration is attached to Snapcaster Mage, which makes it a Vehicle. The
    // opponent then Ousts the Skydiver, the one creature left, and its controller gains life.
    // Each target is the only one when a seat that acts with its own spell on the stack
    // chooses it, so both seats' recordings play the same game.
    Scenario::new("verbs-place")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(THE_WANDERING_EMPEROR),
                Permanent::new(SNAPCASTER_MAGE).face_down(),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[ISLAND, THIEVING_SKYDIVER, SWIFT_RECONFIGURATION])
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(CURRENCY_CONVERTER), ISLAND_, PLAINS_])
        .hand(P1, &[OUST])
        .library(P1, LANDS)
        .actions(&[
            play_land(P0, ISLAND),
            Action::TurnFaceUp {
                player: P0,
                card: SNAPCASTER_MAGE,
            },
            Action::activate(P0, THE_WANDERING_EMPEROR, 1),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            choose(P0, DecisionKind::Target, SNAPCASTER_MAGE),
            cast_with_option(P0, THIEVING_SKYDIVER),
            Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
            choose(P0, DecisionKind::Target, CURRENCY_CONVERTER),
            Action::cast(P0, SWIFT_RECONFIGURATION),
            choose(P0, DecisionKind::Target, SNAPCASTER_MAGE),
            Action::cast(P1, OUST),
            choose(P1, DecisionKind::Target, THIEVING_SKYDIVER),
        ]),
];
