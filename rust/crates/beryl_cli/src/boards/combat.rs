//! Boards of attacks and blocks, and the games they end.

use beryl_cards::names::{
    ABHORRENT_OCULUS, ADELINE_RESPLENDENT_CATHAR, BEZA_THE_BOUNDING_SPRING, COUNTERSPELL,
    CURRENCY_CONVERTER, ELITE_SPELLBINDER, ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY,
    JACE_WIELDER_OF_MYSTERIES, MALCOLM_ALLURING_SCOUNDREL, OVERLORD_OF_THE_MISTMOORS, PLAINS,
    SEACHROME_COAST, SNAPCASTER_MAGE, STAFF_OF_THE_STORYTELLER, STOCK_UP, SUBTLETY,
    SWIFT_RECONFIGURATION, TEFERI_HERO_OF_DOMINARIA, THE_WANDERING_EMPEROR, TRINKET_MAGE, TUNDRA,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{INSECT_TOKEN, ISLAND_, LANDS, PLAINS_, cast_with_option, choose};

/// Boards of attacks and blocks, and the games they end.
pub(super) const COMBAT: &[Scenario] = &[
    // The opponent attacks with Beza, the Bounding Spring into a human at 3 life with nothing
    // to block it, and the game ends on its damage. The opponent has a graveyard and an exile
    // pile, which the way back to the result must stay clear of.
    Scenario::new("game-over")
        .at(3, P0, TurnStep::PrecombatMain)
        .life(P0, 3)
        .battlefield(P0, &[PLAINS_, ISLAND_])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[Permanent::new(BEZA_THE_BOUNDING_SPRING), PLAINS_, ISLAND_],
        )
        .graveyard(P1, &[STOCK_UP])
        .exile(P1, &[MALCOLM_ALLURING_SCOUNDREL])
        .library(P1, LANDS)
        .actions(&[
            // The one creature that may attack ends the declaration.
            choose(P1, DecisionKind::Attacker, BEZA_THE_BOUNDING_SPRING),
        ]),
    // The opponent's Jace, Wielder of Mysteries mills their own last two cards with his +1,
    // and its draw from the empty library wins them the game instead.
    Scenario::new("game-over-jace")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, PLAINS_])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[Permanent::new(JACE_WIELDER_OF_MYSTERIES), ISLAND_, PLAINS_],
        )
        .library(P1, &[ISLAND, PLAINS])
        .actions(&[
            Action::activate(P1, JACE_WIELDER_OF_MYSTERIES, 1),
            Action::answer(P1, DecisionKind::Target, Pick::Player { player: P1 }),
        ]),
    // Six permanents a side, the last face down (the human's tapped): the human's quiet turn,
    // then Trinket Mage, second in its row, blocks Beza, first in hers.
    Scenario::new("combat-blockers")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(COUNTERSPELL).face_down().tapped(),
                PLAINS_,
                ISLAND_,
                PLAINS_,
                ISLAND_,
                Permanent::new(TUNDRA),
            ],
        )
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(SUBTLETY),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(STOCK_UP).face_down(),
                PLAINS_,
                ISLAND_,
                PLAINS_,
                ISLAND_,
                Permanent::new(SEACHROME_COAST),
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::answer(P0, DecisionKind::Attacker, Pick::Done),
            choose(P1, DecisionKind::Attacker, BEZA_THE_BOUNDING_SPRING),
            Action::answer(P1, DecisionKind::Attacker, Pick::Done),
            choose(P0, DecisionKind::Blocker, TRINKET_MAGE),
            Action::answer(P0, DecisionKind::Blocker, Pick::Done),
        ]),
    // Overlord of the Mistmoors' Insects join Elite Spellbinder and Malcolm in an attack two
    // turns on, while Snapcaster Mage stays home.
    Scenario::new("combat-declare")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(SNAPCASTER_MAGE),
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(P0, &[OVERLORD_OF_THE_MISTMOORS])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                PLAINS_,
                ISLAND_,
                ISLAND_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            cast_with_option(P0, OVERLORD_OF_THE_MISTMOORS),
            Action::answer(P0, DecisionKind::Attacker, Pick::Done),
            // In the order the page sends them: by slot.
            choose(P0, DecisionKind::Attacker, MALCOLM_ALLURING_SCOUNDREL),
            choose(P0, DecisionKind::Attacker, ELITE_SPELLBINDER),
            choose(P0, DecisionKind::Attacker, INSECT_TOKEN),
            choose(P0, DecisionKind::Attacker, INSECT_TOKEN),
            Action::answer(P0, DecisionKind::Attacker, Pick::Done),
        ]),
    // A crowded table: Snapcaster Mage and Trinket Mage block Beza, Jace, Vryn's Prodigy
    // blocks Trinket Mage, and Subtlety flies over.
    Scenario::new("combat-double-block")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(ABHORRENT_OCULUS),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                PLAINS_,
                ISLAND_,
                PLAINS_,
                ISLAND_,
                Permanent::new(TUNDRA),
                Permanent::new(SEACHROME_COAST),
            ],
        )
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(SUBTLETY),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                PLAINS_,
                ISLAND_,
                PLAINS_,
                ISLAND_,
                Permanent::new(TUNDRA),
                Permanent::new(SEACHROME_COAST),
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            choose(P1, DecisionKind::Attacker, BEZA_THE_BOUNDING_SPRING),
            Action::answer(P1, DecisionKind::AttackTarget, Pick::DefendingPlayer),
            choose(P1, DecisionKind::Attacker, SUBTLETY),
            Action::answer(P1, DecisionKind::AttackTarget, Pick::DefendingPlayer),
            choose(P1, DecisionKind::Attacker, TRINKET_MAGE),
            Action::answer(P1, DecisionKind::AttackTarget, Pick::DefendingPlayer),
            Action::answer(P1, DecisionKind::Attacker, Pick::Done),
            choose(P0, DecisionKind::Blocker, SNAPCASTER_MAGE),
            choose(P0, DecisionKind::BlockTarget, BEZA_THE_BOUNDING_SPRING),
            choose(P0, DecisionKind::Blocker, TRINKET_MAGE),
            choose(P0, DecisionKind::BlockTarget, BEZA_THE_BOUNDING_SPRING),
            choose(P0, DecisionKind::Blocker, JACE_VRYNS_PRODIGY),
            choose(P0, DecisionKind::BlockTarget, TRINKET_MAGE),
            Action::answer(P0, DecisionKind::Blocker, Pick::Done),
        ]),
    // Snapcaster Mage attacks a player who controls The Wandering Emperor, so the engine asks
    // what it attacks.
    Scenario::new("attack-past-walker")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[Permanent::new(THE_WANDERING_EMPEROR), ISLAND_, PLAINS_],
        )
        .library(P1, LANDS)
        .actions(&[choose(P0, DecisionKind::Attacker, SNAPCASTER_MAGE)]),
    // Adeline attacks a player who controls The Wandering Emperor: her trigger's Human, made
    // attacking, asks what it attacks.
    Scenario::new("attack-token")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[Permanent::new(ADELINE_RESPLENDENT_CATHAR), PLAINS_, PLAINS_],
        )
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[Permanent::new(THE_WANDERING_EMPEROR), ISLAND_, PLAINS_],
        )
        .library(P1, LANDS)
        .actions(&[choose(
            P0,
            DecisionKind::Attacker,
            ADELINE_RESPLENDENT_CATHAR,
        )]),
    // Swift Reconfiguration makes Jace, Vryn's Prodigy a Vehicle with crew 5, which three
    // creatures of power 2, 2 and 3 may crew: Snapcaster Mage, then Elite Spellbinder.
    Scenario::new("crew")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(ELITE_SPELLBINDER),
                PLAINS_,
            ],
        )
        .hand(P0, &[SWIFT_RECONFIGURATION])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, SWIFT_RECONFIGURATION),
            choose(P0, DecisionKind::Target, JACE_VRYNS_PRODIGY),
            Action::activate_granted(P0, JACE_VRYNS_PRODIGY, 0),
            choose(P0, DecisionKind::ChooseObject, SNAPCASTER_MAGE),
            choose(P0, DecisionKind::ChooseObject, ELITE_SPELLBINDER),
        ]),
];
