//! Boards that cast spells and abilities, and pay for them.

use beryl_cards::names::{
    AANGS_ICEBERG, ABHORRENT_OCULUS, COUNTERSPELL, CURRENCY_CONVERTER, DAMN, ELITE_SPELLBINDER,
    ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY, JACE_WIELDER_OF_MYSTERIES,
    MALCOLM_ALLURING_SCOUNDREL, MANA_CRYPT, MANA_TITHE, NO_MORE_LIES, OUST, PLAINS, PORTABLE_HOLE,
    REPRIEVE, SINK_INTO_STUPOR, SNAPCASTER_MAGE, SPARAS_HEADQUARTERS, SPHINX_OF_FALSE_CONCLUSIONS,
    STAFF_OF_THE_STORYTELLER, STOCK_UP, SUBTLETY, SWIFT_RECONFIGURATION, TEFERI_HERO_OF_DOMINARIA,
    TEFERI_TIME_RAVELER, THE_WANDERING_EMPEROR, THIEVING_SKYDIVER, THIRST_FOR_DISCOVERY,
    THUNDERTRAP_TRAINER, TRINKET_MAGE, TUNDRA,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{ISLAND_, LANDS, PLAINS_, cast_with_option, choose, play_land};
use super::tables::converter;

/// Boards that cast spells and abilities, and pay for them.
pub(super) const STACK: &[Scenario] = &[
    // The opponent flashes in Snapcaster Mage, aiming its trigger at No More Lies.
    Scenario::new("stack-trigger")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THIRST_FOR_DISCOVERY, REPRIEVE])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(P1, &[SNAPCASTER_MAGE])
        .graveyard(P1, &[NO_MORE_LIES, STOCK_UP])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P1, SNAPCASTER_MAGE),
            choose(P1, DecisionKind::Target, NO_MORE_LIES),
        ]),
    // The opponent casts Stock Up and the human answers with Reprieve, which returns it to the
    // opponent's hand and draws a card.
    Scenario::new("stack-bounce")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[REPRIEVE])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(P1, &[STOCK_UP])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P1, STOCK_UP),
            Action::cast(P0, REPRIEVE),
            choose(P0, DecisionKind::Target, STOCK_UP),
        ]),
    // No More Lies exiles the human's Jace, Wielder of Mysteries, who declines to pay; in the
    // opponent's turn the human holds Counterspell over their Stock Up.
    Scenario::new("stack-counter")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                ISLAND_, ISLAND_, ISLAND_, ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_,
            ],
        )
        .hand(P0, &[JACE_WIELDER_OF_MYSTERIES, COUNTERSPELL])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(P1, &[NO_MORE_LIES, STOCK_UP])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, JACE_WIELDER_OF_MYSTERIES),
            Action::cast(P1, NO_MORE_LIES),
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            Action::cast(P1, STOCK_UP),
        ]),
    // Thieving Skydiver cast with kicker from a full hand, its X yet to choose, with
    // artifacts to steal.
    Scenario::new("cast-kicker")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(
            P0,
            &[
                THE_WANDERING_EMPEROR,
                OUST,
                SPARAS_HEADQUARTERS,
                PLAINS,
                THIEVING_SKYDIVER,
                COUNTERSPELL,
                MALCOLM_ALLURING_SCOUNDREL,
                ISLAND,
            ],
        )
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(MANA_CRYPT),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[cast_with_option(P0, THIEVING_SKYDIVER)]),
    // Subtlety cast with evoke, the blue card to exile from the hand yet to choose; it could be
    // cast for its mana too.
    Scenario::new("cast-evoke")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(P0, &[SUBTLETY, COUNTERSPELL, JACE_VRYNS_PRODIGY, PLAINS])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[cast_with_option(P0, SUBTLETY)]),
    // Teferi denies an instant during upkeep; a creature without flash has its own timing limit.
    Scenario::new("teferi-timing")
        .at(3, P0, TurnStep::Upkeep)
        .battlefield(P0, &[ISLAND_, ISLAND_, ISLAND_])
        .hand(P0, &[THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER])
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(TEFERI_TIME_RAVELER)])
        .library(P1, LANDS),
    // Portable Hole takes the opponent's Snapcaster Mage, beside a tapped land, an untapped
    // one and a tapped creature.
    Scenario::new("exiled-beneath")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(TRINKET_MAGE).tapped(),
                PLAINS_,
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(P0, &[PORTABLE_HOLE, ISLAND])
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, PORTABLE_HOLE)]),
    // Swift Reconfiguration on the opponent's Sphinx of False Conclusions, beside the human's
    // Teferi, Hero of Dominaria with two loyalty abilities to offer.
    Scenario::new("attached-across")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(TRINKET_MAGE),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                PLAINS_,
                ISLAND_,
            ],
        )
        .hand(P0, &[SWIFT_RECONFIGURATION, ISLAND])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(SPHINX_OF_FALSE_CONCLUSIONS),
                PLAINS_,
                ISLAND_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, SWIFT_RECONFIGURATION),
            choose(P0, DecisionKind::Target, SPHINX_OF_FALSE_CONCLUSIONS),
        ]),
    // The opponent's Elite Spellbinder exiles the human's Teferi, Time Raveler; Sink into
    // Stupor returns the Spellbinder to their hand, and the human may cast Teferi from exile.
    Scenario::new("exile-cast")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                Permanent::new(TUNDRA),
            ],
        )
        .hand(P0, &[TEFERI_TIME_RAVELER, SINK_INTO_STUPOR, ISLAND])
        .library(P0, LANDS)
        .battlefield(P1, &[PLAINS_, PLAINS_, ISLAND_, ISLAND_])
        .hand(P1, &[ELITE_SPELLBINDER, THIEVING_SKYDIVER, OUST])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P1, ELITE_SPELLBINDER),
            Action::pass(P0),
            Action::cast(P0, SINK_INTO_STUPOR),
            Action::answer(P1, DecisionKind::YesNo, Pick::Yes),
        ]),
    // Snapcaster Mage gives the human's Thirst for Discovery flashback: castable from the
    // graveyard, it waits in their hand fan, while Counterspell stays in the pile.
    Scenario::new("flashback")
        .at(4, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, ISLAND_, ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[SNAPCASTER_MAGE, ISLAND])
        .graveyard(P0, &[COUNTERSPELL, THIRST_FOR_DISCOVERY])
        .library(P0, LANDS)
        .battlefield(P1, &[PLAINS_, ISLAND_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, SNAPCASTER_MAGE),
            choose(P0, DecisionKind::Target, THIRST_FOR_DISCOVERY),
        ]),
    // Counterspell can answer Mana Tithe or the human's Mana Crypt below it.
    Scenario::new("counterspell-targets")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_])
        .hand(P0, &[MANA_CRYPT, COUNTERSPELL])
        .library(P0, LANDS)
        .battlefield(P1, &[PLAINS_])
        .hand(P1, &[MANA_TITHE])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, MANA_CRYPT),
            Action::cast(P1, MANA_TITHE),
            Action::cast(P0, COUNTERSPELL),
        ]),
    // Reprieve has one target and can pay the generic mana with either a land or Mana Crypt.
    Scenario::new("reprieve-target")
        .at(3, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[PLAINS_, ISLAND_, Permanent::new(MANA_CRYPT)])
        .hand(P0, &[REPRIEVE])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P1, &[TEFERI_TIME_RAVELER])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P1, TEFERI_TIME_RAVELER),
            Action::cast(P0, REPRIEVE),
        ]),
    // Mana Crypt pays for Currency Converter and leaves {C} floating at the human's priority;
    // a land in hand keeps it theirs.
    Scenario::new("mana-floating")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(MANA_CRYPT)])
        .hand(P0, &[CURRENCY_CONVERTER, ISLAND])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, CURRENCY_CONVERTER)]),
    // The human's own main phase with a Tundra, an Island and a Plains untapped and Reprieve
    // to cast: tapping the Tundra asks which mana it adds.
    Scenario::new("mana-picker")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(TUNDRA), ISLAND_, PLAINS_])
        .hand(P0, &[REPRIEVE])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS),
    // Currency Converter is asked whether to exile Thirst for Discovery's first discard; has
    // exiled both; and puts one of the two back.
    converter(
        "converter-may-exile",
        &[
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            choose(P0, DecisionKind::ChooseObject, TUNDRA),
            choose(P0, DecisionKind::ChooseObject, COUNTERSPELL),
        ],
    ),
    converter(
        "converter-exiled",
        &[
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            choose(P0, DecisionKind::ChooseObject, TUNDRA),
            choose(P0, DecisionKind::ChooseObject, COUNTERSPELL),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
        ],
    ),
    converter(
        "converter-choose",
        &[
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            choose(P0, DecisionKind::ChooseObject, TUNDRA),
            choose(P0, DecisionKind::ChooseObject, COUNTERSPELL),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::activate(P0, CURRENCY_CONVERTER, 2),
        ],
    ),
    // Counterspell over a Thieving Skydiver kicked for X=1: a stack of both players' items,
    // one with an X.
    Scenario::new("stack-x")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_, PLAINS_])
        .hand(P0, &[THIEVING_SKYDIVER, REPRIEVE])
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(MANA_CRYPT), ISLAND_, ISLAND_])
        .hand(P1, &[COUNTERSPELL])
        .library(P1, LANDS)
        .actions(&[
            cast_with_option(P0, THIEVING_SKYDIVER),
            Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
            Action::cast(P1, COUNTERSPELL),
        ]),
    // The opponent's Damn cast for its overload cost, destroying each creature rather than a
    // target, as the human gets priority over it, holding a Counterspell.
    Scenario::new("stack-overload")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(P0, &[COUNTERSPELL])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(ELITE_SPELLBINDER),
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P1, &[DAMN])
        .library(P1, LANDS)
        .actions(&[cast_with_option(P1, DAMN)]),
    // Sink into Stupor played as Soporific Springs beside three untapped lands, with Jace, the
    // Mind Sculptor the card a fourth would make castable.
    Scenario::new("pay-life")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[SINK_INTO_STUPOR, JACE_THE_MIND_SCULPTOR])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[play_land(P0, SINK_INTO_STUPOR)]),
    // Staff of the Storyteller cast beside Mana Crypt, which pays it a second way, leaving a
    // mana floating; the human pays the default way, with lands.
    Scenario::new("pay-default")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(MANA_CRYPT), ISLAND_, PLAINS_])
        .hand(P0, &[STAFF_OF_THE_STORYTELLER, ISLAND])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, STAFF_OF_THE_STORYTELLER)]),
    // With exactly six cards in the graveyard, Oculus pays its exile cost without a choice.
    Scenario::new("cast-oculus")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[ABHORRENT_OCULUS])
        .graveyard(
            P0,
            &[PLAINS, ISLAND, OUST, REPRIEVE, COUNTERSPELL, STOCK_UP],
        )
        .library(P0, LANDS)
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, ABHORRENT_OCULUS)]),
    // A seventh graveyard card leaves a live additional-cost choice after mana payment.
    Scenario::new("cast-exile-choice")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[ABHORRENT_OCULUS])
        .graveyard(
            P0,
            &[
                PLAINS,
                ISLAND,
                OUST,
                REPRIEVE,
                COUNTERSPELL,
                STOCK_UP,
                THIRST_FOR_DISCOVERY,
            ],
        )
        .library(P0, LANDS)
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, ABHORRENT_OCULUS),
            Action::choose(P0, DecisionKind::ChooseObject, THIRST_FOR_DISCOVERY),
        ]),
    // Aang's Iceberg's waterbend {3} with one Plains: the Staff of the Storyteller or Jace,
    // Vryn's Prodigy taps for one of it.
    Scenario::new("waterbend-tap")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(AANGS_ICEBERG),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(JACE_VRYNS_PRODIGY),
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, AANGS_ICEBERG, 1)]),
];
