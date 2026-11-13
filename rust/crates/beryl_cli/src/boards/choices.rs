//! Boards that put a choice to the human.

use beryl_cards::names::{
    AANGS_ICEBERG, COUNTERSPELL, CURRENCY_CONVERTER, DAMN, ELITE_SPELLBINDER, GET_LOST, ISLAND,
    JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY, MALCOLM_ALLURING_SCOUNDREL, MANA_CRYPT, MANA_DRAIN,
    MANA_TITHE, OUST, PLAINS, REPRIEVE, SINK_INTO_STUPOR, SNAPCASTER_MAGE, STOCK_UP,
    SWIFT_RECONFIGURATION, TEFERI_HERO_OF_DOMINARIA, TEFERI_TIME_RAVELER, THIRST_FOR_DISCOVERY,
    THUNDERTRAP_TRAINER, TIME_SPIRAL, TRINKET_MAGE, WINDS_OF_ABANDON,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{ISLAND_, LANDS, MAP_TOKEN, PLAINS_};

/// Boards that put a choice to the human.
pub(super) const CHOICES: &[Scenario] = &[
    // Aang's Iceberg is all the human uses; both players have cards in every pile, and the
    // human a hand to fan out and a creature that may attack.
    Scenario::new("choice-scry")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(AANGS_ICEBERG),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
                Permanent::new(ISLAND),
            ],
        )
        .hand(
            P0,
            &[
                JACE_THE_MIND_SCULPTOR,
                TEFERI_HERO_OF_DOMINARIA,
                DAMN,
                COUNTERSPELL,
                STOCK_UP,
                THIRST_FOR_DISCOVERY,
            ],
        )
        .graveyard(P0, &[OUST, MANA_DRAIN])
        .exile(P0, &[TIME_SPIRAL])
        .library(P0, &[COUNTERSPELL, ISLAND, PLAINS, ISLAND])
        .permanents(P1, &[Permanent::new(ISLAND), Permanent::new(PLAINS)])
        .graveyard(P1, &[SWIFT_RECONFIGURATION])
        .exile(P1, &[MALCOLM_ALLURING_SCOUNDREL])
        .library(P1, LANDS)
        // Lands pay the waterbend, not the Snapcaster Mage, unasked.
        .actions(&[Action::activate(P0, AANGS_ICEBERG, 1)]),
    // Jace, the Mind Sculptor's brainstorm, putting back two of a full hand.
    Scenario::new("choice-jace")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                PLAINS_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
            ],
        )
        .hand(
            P0,
            &[COUNTERSPELL, TEFERI_TIME_RAVELER, WINDS_OF_ABANDON, PLAINS],
        )
        .library(P0, &[ISLAND, OUST, DAMN, PLAINS, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, JACE_THE_MIND_SCULPTOR, 1)]),
    // Thundertrap Trainer looks at the top four for a spell.
    Scenario::new("choice-look")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THUNDERTRAP_TRAINER])
        .library(P0, &[ISLAND, COUNTERSPELL, PLAINS, DAMN, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, THUNDERTRAP_TRAINER)]),
    // Mana Tithe on Thundertrap Trainer, with two lands left to pay.
    Scenario::new("choice-pay")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(P0, &[THUNDERTRAP_TRAINER])
        .library(P0, LANDS)
        .permanents(P1, &[PLAINS_, ISLAND_])
        .hand(P1, &[MANA_TITHE])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THUNDERTRAP_TRAINER),
            Action::cast(P1, MANA_TITHE),
        ]),
    // Jace, the Mind Sculptor's three affordable loyalty abilities, and Sink into Stupor both
    // castable and playable as its land face.
    Scenario::new("menu-choices")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                ISLAND_,
                ISLAND_,
                PLAINS_,
            ],
        )
        .hand(P0, &[SINK_INTO_STUPOR, COUNTERSPELL])
        .library(P0, LANDS)
        .permanents(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS),
    // Get Lost destroys one of the human's Elite Spellbinders, leaving them two Maps; one
    // makes the other Spellbinder explore, revealing Oust.
    Scenario::new("choice-explore")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(ELITE_SPELLBINDER),
                PLAINS_,
                PLAINS_,
                ISLAND_,
            ],
        )
        .hand(P0, &[GET_LOST])
        .library(P0, &[OUST, ISLAND, PLAINS, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, GET_LOST),
            Action::activate(P0, MAP_TOKEN, 0),
        ]),
    // Jace, Vryn's Prodigy loots: it draws Stock Up, then asks for the discard.
    Scenario::new("choice-loot")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(JACE_VRYNS_PRODIGY), ISLAND_, PLAINS_])
        .hand(P0, &[COUNTERSPELL, OUST])
        .library(P0, &[STOCK_UP, ISLAND, PLAINS, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, JACE_VRYNS_PRODIGY, 0)]),
    // The same with a full hand: eight cards to discard from, as wide as a hand is held,
    // beside the ability that asks on the stack.
    Scenario::new("choice-loot-full")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(JACE_VRYNS_PRODIGY), ISLAND_, PLAINS_])
        .hand(
            P0,
            &[
                COUNTERSPELL,
                OUST,
                DAMN,
                REPRIEVE,
                MANA_DRAIN,
                ISLAND,
                PLAINS,
            ],
        )
        .library(P0, &[STOCK_UP, ISLAND, PLAINS, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, JACE_VRYNS_PRODIGY, 0)]),
    // Time Spiral, paid with Mana Crypt and the human's lands, untaps up to six lands, more
    // than the human has tapped, beside the opponent's tapped and untapped ones.
    Scenario::new("choice-untap")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(MANA_CRYPT),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[TIME_SPIRAL])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(ISLAND).tapped(),
                Permanent::new(PLAINS).tapped(),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, TIME_SPIRAL)]),
    // Teferi, Hero of Dominaria's +1 untaps up to two lands at the end step, of the human's
    // two tapped lands.
    Scenario::new("choice-teferi")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(ISLAND).tapped(),
                Permanent::new(PLAINS).tapped(),
                ISLAND_,
            ],
        )
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, TEFERI_HERO_OF_DOMINARIA, 0)]),
    // Trinket Mage searches for up to one artifact of mana value 1 or less, of two; the smart
    // stops search, as the human knows nothing of their library.
    Scenario::new("choice-search")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[TRINKET_MAGE])
        .library(
            P0,
            &[MANA_CRYPT, ISLAND, CURRENCY_CONVERTER, PLAINS, ISLAND],
        )
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, TRINKET_MAGE)]),
    // Thirst for Discovery draws three, and the human discards one of two basic lands rather
    // than two cards.
    Scenario::new("choice-thirst")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THIRST_FOR_DISCOVERY, PLAINS, ISLAND, COUNTERSPELL])
        .library(P0, &[STOCK_UP, OUST, MANA_DRAIN, ISLAND, PLAINS])
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
        ]),
    // The human ends their turn with eight cards in hand, one over the maximum.
    Scenario::new("choice-cleanup")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, PLAINS_])
        .hand(
            P0,
            &[
                COUNTERSPELL,
                OUST,
                MANA_DRAIN,
                STOCK_UP,
                DAMN,
                ISLAND,
                PLAINS,
                REPRIEVE,
            ],
        )
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS),
];
