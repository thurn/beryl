//! The boards the verb suite watches (`ui/verbs.json`, `ui/scripts/verbs.mjs`): each plays the
//! changes a verb's check asks for, recorded from both seats.

use beryl_cards::names::{
    AANGS_ICEBERG, ADELINE_RESPLENDENT_CATHAR, BEZA_THE_BOUNDING_SPRING, COUNTERSPELL,
    CURRENCY_CONVERTER, DAMN, ELITE_SPELLBINDER, ISLAND, JACE_THE_MIND_SCULPTOR,
    JACE_WIELDER_OF_MYSTERIES, MANA_CRYPT, OUST, OVERLORD_OF_THE_MISTMOORS, PLAINS, PORTABLE_HOLE,
    REPRIEVE, SNAPCASTER_MAGE, STOCK_UP, SWIFT_RECONFIGURATION, TEFERI_TIME_RAVELER,
    THE_WANDERING_EMPEROR, THIEVING_SKYDIVER, THIRST_FOR_DISCOVERY, TRINKET_MAGE,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{
    INSECT_TOKEN, ISLAND_, LANDS, PLAINS_, TREASURE_TOKEN, cast_with_option, choose, play_land,
};

/// A library for the many draws of verbs-piles: Jace, Wielder of Mysteries wins its controller
/// the game on a draw from an empty one.
const DRAWS: &[&str] = &[
    ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS,
    ISLAND, PLAINS, ISLAND, PLAINS,
];

/// What the first seat scries with: Aang's Iceberg and the lands to activate it.
const ICEBERG: &[Permanent] = &[Permanent::new(AANGS_ICEBERG), PLAINS_, PLAINS_, ISLAND_];

/// The first seat's table for a scry of two with Aang's Iceberg, named `name`.
const fn scry(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, ICEBERG)
        .hand(P0, &[COUNTERSPELL])
        .library(P0, &[COUNTERSPELL, ISLAND, PLAINS, ISLAND])
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
}

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
    // Cards leaving and entering piles the other ways: the first seat's Snapcaster Mage gives
    // the Thirst for Discovery in its graveyard flashback, and it casts Thirst from there,
    // draws three, discards a land and exiles Thirst; it exiles the opponent's Mana Crypt
    // under Portable Hole; its Elite Spellbinder exiles their Teferi, Time Raveler from their
    // hand; its Jace, Wielder of Mysteries mills it two and draws; and Jace, the Mind Sculptor
    // draws it three and puts two back. In their turn the
    // opponent casts Teferi from exile and returns Portable Hole to its owner's hand with it,
    // so Mana Crypt comes back, and attacks with Adeline, whose Human token enters attacking.
    Scenario::new("verbs-piles")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(JACE_WIELDER_OF_MYSTERIES),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[SNAPCASTER_MAGE, PORTABLE_HOLE, ELITE_SPELLBINDER])
        .graveyard(P0, &[THIRST_FOR_DISCOVERY])
        .library(P0, DRAWS)
        .battlefield(
            P1,
            &[
                Permanent::new(ADELINE_RESPLENDENT_CATHAR),
                Permanent::new(MANA_CRYPT),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P1, &[TEFERI_TIME_RAVELER, ISLAND])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, SNAPCASTER_MAGE),
            choose(P0, DecisionKind::Target, THIRST_FOR_DISCOVERY),
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            choose(P0, DecisionKind::ChooseObject, ISLAND),
            Action::cast(P0, PORTABLE_HOLE),
            choose(P0, DecisionKind::Target, MANA_CRYPT),
            Action::cast(P0, ELITE_SPELLBINDER),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            // Last of its turn: a seat with its own ability on the stack and a spell still to
            // cast would cast it in answer from the chair the session does not pass for.
            Action::activate(P0, JACE_WIELDER_OF_MYSTERIES, 1),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P0 }),
            Action::activate(P0, JACE_THE_MIND_SCULPTOR, 1),
            choose(P0, DecisionKind::ChooseObject, PLAINS),
            choose(P0, DecisionKind::ChooseObject, PLAINS),
            Action::cast(P1, TEFERI_TIME_RAVELER),
            Action::activate(P1, TEFERI_TIME_RAVELER, 2),
            Action::answer(P1, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            choose(P1, DecisionKind::Target, PORTABLE_HOLE),
            choose(P1, DecisionKind::Attacker, ADELINE_RESPLENDENT_CATHAR),
            Action::answer(P1, DecisionKind::AttackTarget, Pick::DefendingPlayer),
            // The token's: it attacks the player, not their planeswalker.
            Action::answer(P1, DecisionKind::AttackTarget, Pick::DefendingPlayer),
        ]),
    // The first seat scries two with Aang's Iceberg and puts one of them on the bottom.
    scry("verbs-scry").actions(&[
        Action::activate(P0, AANGS_ICEBERG, 1),
        choose(P0, DecisionKind::ChooseUpTo, ISLAND),
        Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
    ]),
    // The same scry left open: the first seat looks at its two cards and has yet to choose,
    // so a recording stops while the look lasts (the stepped capture's lift).
    scry("verbs-look").actions(&[Action::activate(P0, AANGS_ICEBERG, 1)]),
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
