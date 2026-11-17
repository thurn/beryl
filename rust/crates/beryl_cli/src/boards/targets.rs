//! Boards that choose targets, and the arrows drawn to them.

use beryl_cards::names::{
    ANIMATE_DEAD, COUNTERSPELL, FORK, ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY,
    JACE_WIELDER_OF_MYSTERIES, LIGHTNING_BOLT, MANA_CRYPT, MOUNTAIN, OUST, PLAINS, REPRIEVE,
    SNAPCASTER_MAGE, STAFF_OF_THE_STORYTELLER, STOCK_UP, SWAMP, TEFERI_HERO_OF_DOMINARIA,
    TEFERI_TIME_RAVELER, THIRST_FOR_DISCOVERY, TORMODS_CRYPT, TRINKET_MAGE,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{ISLAND_, LANDS, PLAINS_, choose};

/// Boards that choose targets, and the arrows drawn to them.
pub(super) const TARGETS: &[Scenario] = &[
    // Counterspell aimed at the human's Trinket Mage, which the human may save.
    Scenario::new("arrow-spell")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_, PLAINS_])
        .hand(P0, &[TRINKET_MAGE, REPRIEVE])
        .library(P0, &[MANA_CRYPT, ISLAND, PLAINS, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, ISLAND_])
        .hand(P1, &[COUNTERSPELL])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, TRINKET_MAGE),
            Action::cast(P1, COUNTERSPELL),
        ]),
    // The human's Oust aimed at the opponent's Trinket Mage, under the opponent's Counterspell
    // aimed at Oust, which the human may save: Oust's arrow still points to the battlefield
    // even though the stack itself needs no arrow between its two spells.
    Scenario::new("arrow-stacked")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[PLAINS_, PLAINS_, PLAINS_, ISLAND_])
        .hand(P0, &[OUST, REPRIEVE])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(TRINKET_MAGE),
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(P1, &[COUNTERSPELL])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, OUST),
            choose(P0, DecisionKind::Target, TRINKET_MAGE),
            Action::cast(P1, COUNTERSPELL),
        ]),
    // The opponent's Jace, the Mind Sculptor returns their own Trinket Mage, beside it.
    Scenario::new("arrow-ability")
        .at(4, P1, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, ISLAND_, PLAINS_],
        )
        .hand(P0, &[THIRST_FOR_DISCOVERY])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(TRINKET_MAGE),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P1, JACE_THE_MIND_SCULPTOR, 2),
            choose(P1, DecisionKind::Target, TRINKET_MAGE),
        ]),
    // The opponent's Jace, Wielder of Mysteries has the human mill two.
    Scenario::new("arrow-player")
        .at(4, P1, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THIRST_FOR_DISCOVERY])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[Permanent::new(JACE_WIELDER_OF_MYSTERIES), ISLAND_, PLAINS_],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P1, JACE_WIELDER_OF_MYSTERIES, 1),
            Action::answer(P1, DecisionKind::Target, Pick::Player { player: P0 }),
        ]),
    // The human's Jace, Wielder of Mysteries asks which player its +1 targets.
    Scenario::new("target-player")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[Permanent::new(JACE_WIELDER_OF_MYSTERIES), ISLAND_, PLAINS_],
        )
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, JACE_WIELDER_OF_MYSTERIES, 1)]),
    // Teferi, Hero of Dominaria's −3 with a Snapcaster Mage on each side.
    Scenario::new("namesake-target")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .permanents(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, TEFERI_HERO_OF_DOMINARIA, 1)]),
    // Teferi, Time Raveler's −3 announces how many targets, up to one, with targets on both sides; the human then announces one.
    Scenario::new("target-up-to-one")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TEFERI_TIME_RAVELER),
                Permanent::new(JACE_VRYNS_PRODIGY),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P0, TEFERI_TIME_RAVELER, 2),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
        ]),
    // Oust with Jace, Vryn's Prodigy the only creature: its one legal target is the human's
    // own, which they are still asked to choose.
    Scenario::new("target-own")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(JACE_VRYNS_PRODIGY), PLAINS_, ISLAND_])
        .hand(P0, &[OUST])
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, OUST)]),
    // Snapcaster Mage's trigger targets an instant or sorcery card in the human's graveyard,
    // of three beside a creature card it can't target.
    Scenario::new("target-graveyard")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[SNAPCASTER_MAGE])
        .graveyard(
            P0,
            &[COUNTERSPELL, TRINKET_MAGE, THIRST_FOR_DISCOVERY, STOCK_UP],
        )
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, SNAPCASTER_MAGE)]),
    // Animate Dead aimed at Snapcaster Mage in the opponent's graveyard, which the opponent
    // exiles in response with Tormod's Crypt, so the spell's arrow ends and Animate Dead
    // finds no target. The land in hand keeps the human's priority after it.
    Scenario::new("arrow-target-leaves")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[SWAMP, PLAINS])
        .hand(P0, &[ANIMATE_DEAD, ISLAND])
        .library(P0, LANDS)
        .permanents(P1, &[Permanent::new(TORMODS_CRYPT), ISLAND_])
        .graveyard(P1, &[SNAPCASTER_MAGE])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, ANIMATE_DEAD),
            Action::activate(P1, TORMODS_CRYPT, 0),
            Action::answer(P1, DecisionKind::Target, Pick::Player { player: P1 }),
        ]),
    // The opponent's Lightning Bolt aims at the human, who copies it with Fork and aims the
    // copy at the opponent: the copy is on the stack above the Bolt. A second Bolt and the
    // mana for it, and the fixture's full control, give the human priority there.
    Scenario::new("stack-copy")
        .at(3, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN])
        .hand(P0, &[FORK, LIGHTNING_BOLT])
        .library(P0, LANDS)
        .battlefield(P1, &[MOUNTAIN])
        .hand(P1, &[LIGHTNING_BOLT])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P1, LIGHTNING_BOLT),
            Action::answer(P1, DecisionKind::Target, Pick::Player { player: P0 }),
            Action::cast(P0, FORK),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
        ]),
];
