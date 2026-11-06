//! The boards UI fixtures start from: each a scenario's setup, and the actions that play it
//! into the moment its fixture shows. A [`Scripted`] agent in each seat takes that seat's
//! actions in order as the session asks its decisions, so a fixture built on a board shows the
//! same game however the agents are tuned or the decks change.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use beryl_cards::names::{
    AANGS_ICEBERG, ABHORRENT_OCULUS, ADELINE_RESPLENDENT_CATHAR, BEZA_THE_BOUNDING_SPRING,
    COUNTERSPELL, CURRENCY_CONVERTER, DAMN, ELITE_SPELLBINDER, GET_LOST, ISLAND,
    JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY, JACE_WIELDER_OF_MYSTERIES,
    MALCOLM_ALLURING_SCOUNDREL, MANA_CRYPT, MANA_DRAIN, MANA_TITHE, NO_MORE_LIES, OUST,
    OVERLORD_OF_THE_MISTMOORS, PLAINS, PORTABLE_HOLE, REPRIEVE, SEACHROME_COAST, SINK_INTO_STUPOR,
    SNAPCASTER_MAGE, SPARAS_HEADQUARTERS, SPHINX_OF_FALSE_CONCLUSIONS, STAFF_OF_THE_STORYTELLER,
    STOCK_UP, SUBTLETY, SWIFT_RECONFIGURATION, TEFERI_HERO_OF_DOMINARIA, TEFERI_TIME_RAVELER,
    THE_WANDERING_EMPEROR, THIEVING_SKYDIVER, THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER,
    TIME_SPIRAL, TRINKET_MAGE, TUNDRA, WINDS_OF_ABANDON,
};
use beryl_engine::card::{Castings, FaceIx};
use beryl_engine::state::CounterKind;
use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Observation, Seat, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};
use beryl_search::Agent;

/// The tokens the boards name, as the cards that make them name them.
const INSECT_TOKEN: &str = "Insect Token";
const MAP_TOKEN: &str = "Map Token";

/// Every board, by name.
pub const BOARDS: &[Scenario] = &[
    // Aang's Iceberg is all the human uses; both players have cards in every pile, and the
    // human a hand to fan out and a creature that may attack.
    Scenario::new("choice-scry")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
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
        .battlefield(P1, &[Permanent::new(ISLAND), Permanent::new(PLAINS)])
        .graveyard(P1, &[SWIFT_RECONFIGURATION])
        .exile(P1, &[MALCOLM_ALLURING_SCOUNDREL])
        .library(P1, LANDS)
        // Lands pay the waterbend, not the Snapcaster Mage, unasked.
        .actions(&[Action::activate(P0, AANGS_ICEBERG, 1)]),
    // The human casts Thundertrap Trainer and takes nothing; in the opponent's turn they play
    // a land and Stock Up, and the human has nothing to do until their next main phase.
    Scenario::new("opponent-turn")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THUNDERTRAP_TRAINER])
        .library(
            P0,
            &[
                COUNTERSPELL,
                DAMN,
                OUST,
                JACE_THE_MIND_SCULPTOR,
                TEFERI_HERO_OF_DOMINARIA,
                TEFERI_HERO_OF_DOMINARIA,
                JACE_THE_MIND_SCULPTOR,
            ],
        )
        .battlefield(P1, &[ISLAND_, ISLAND_, ISLAND_])
        .hand(P1, &[PLAINS, STOCK_UP])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THUNDERTRAP_TRAINER),
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            play_land(P1, PLAINS),
            Action::cast(P1, STOCK_UP),
        ]),
    // An opening hand holding two Plains.
    Scenario::new("hand-pair")
        .hand(
            P0,
            &[
                SNAPCASTER_MAGE,
                PLAINS,
                ISLAND,
                MANA_CRYPT,
                PLAINS,
                ABHORRENT_OCULUS,
                TEFERI_TIME_RAVELER,
            ],
        )
        .library(P0, LANDS)
        .hand(P1, &[ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS])
        .library(P1, LANDS),
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
    // Jace, the Mind Sculptor's brainstorm, putting back two of a full hand.
    Scenario::new("choice-jace")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
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
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, JACE_THE_MIND_SCULPTOR, 1)]),
    // Thundertrap Trainer looks at the top four for a spell.
    Scenario::new("choice-look")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THUNDERTRAP_TRAINER])
        .library(P0, &[ISLAND, COUNTERSPELL, PLAINS, DAMN, ISLAND, PLAINS])
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, THUNDERTRAP_TRAINER)]),
    // Mana Tithe on Thundertrap Trainer, with two lands left to pay.
    Scenario::new("choice-pay")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_, PLAINS_])
        .hand(P0, &[THUNDERTRAP_TRAINER])
        .library(P0, LANDS)
        .battlefield(P1, &[PLAINS_, ISLAND_])
        .hand(P1, &[MANA_TITHE])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THUNDERTRAP_TRAINER),
            Action::cast(P1, MANA_TITHE),
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
    // Counterspell aimed at the human's Trinket Mage, which the human may save.
    Scenario::new("arrow-spell")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_, PLAINS_])
        .hand(P0, &[TRINKET_MAGE, REPRIEVE])
        .library(P0, &[MANA_CRYPT, ISLAND, PLAINS, ISLAND, PLAINS])
        .battlefield(P1, &[ISLAND_, ISLAND_])
        .hand(P1, &[COUNTERSPELL])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, TRINKET_MAGE),
            Action::cast(P1, COUNTERSPELL),
        ]),
    // The opponent's Jace, the Mind Sculptor returns their own Trinket Mage, beside it.
    Scenario::new("arrow-ability")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, ISLAND_, PLAINS_],
        )
        .hand(P0, &[THIRST_FOR_DISCOVERY])
        .library(P0, LANDS)
        .battlefield(
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
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THIRST_FOR_DISCOVERY])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[Permanent::new(JACE_WIELDER_OF_MYSTERIES), ISLAND_, PLAINS_],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P1, JACE_WIELDER_OF_MYSTERIES, 1),
            Action::answer(P1, DecisionKind::Target, Pick::Player { player: P0 }),
        ]),
    // Twins with one mark between them: +1/+1 counters, a story counter, and a Thieving
    // Skydiver just cast beside one that has been there.
    Scenario::new("marks")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(TRINKET_MAGE).counters(CounterKind::PlusOne, 2),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 1),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(THIEVING_SKYDIVER),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[THIEVING_SKYDIVER])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, THIEVING_SKYDIVER)]),
    // Subtlety blocks one of two Abhorrent Oculi and leaves it damaged; a land to play keeps
    // the human's priority after combat.
    Scenario::new("marks-damage")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(ABHORRENT_OCULUS),
                Permanent::new(ABHORRENT_OCULUS),
                ISLAND_,
                ISLAND_,
                PLAINS_,
            ],
        )
        .hand(P0, &[PLAINS])
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(SUBTLETY), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            choose(P0, DecisionKind::Attacker, ABHORRENT_OCULUS),
            Action::answer(P0, DecisionKind::Attacker, Pick::Done),
            choose(P1, DecisionKind::Blocker, SUBTLETY),
        ]),
    // Get Lost on the human's own Snapcaster Mage makes two Map tokens; Overlord of the
    // Mistmoors' Insects, one of them under Swift Reconfiguration; and a kicked Thieving
    // Skydiver steals Mana Crypt.
    Scenario::new("tokens")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(
            P0,
            &[
                GET_LOST,
                OVERLORD_OF_THE_MISTMOORS,
                SWIFT_RECONFIGURATION,
                THIEVING_SKYDIVER,
            ],
        )
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(MANA_CRYPT), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, GET_LOST),
            cast_with_option(P0, OVERLORD_OF_THE_MISTMOORS),
            Action::cast(P0, SWIFT_RECONFIGURATION),
            cast_with_option(P0, THIEVING_SKYDIVER),
            Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
            choose(P0, DecisionKind::Target, MANA_CRYPT),
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
    // Jace, the Mind Sculptor's three affordable loyalty abilities, and Sink into Stupor both
    // castable and playable as its land face.
    Scenario::new("menu-choices")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
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
        .battlefield(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS),
    // Teferi, Time Raveler at 1 loyalty: its +1 affordable, its −3 not, though the opponent's
    // Snapcaster Mage could be its target.
    Scenario::new("walker-low")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(TEFERI_TIME_RAVELER).counters(CounterKind::Loyalty, 1),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS),
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
    // Teferi, Hero of Dominaria's −3 with a Snapcaster Mage on each side.
    Scenario::new("namesake-target")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .battlefield(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, TEFERI_HERO_OF_DOMINARIA, 1)]),
    // A crowded table, nine and ten nonland permanents a side, among them planeswalkers, a pile
    // of twins, counters and a tapped one: the opponent casts Stock Up, which the human may
    // counter.
    Scenario::new("crowded")
        .at(4, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(THUNDERTRAP_TRAINER),
                Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 2),
                Permanent::new(CURRENCY_CONVERTER).tapped(),
                Permanent::new(MANA_CRYPT),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                Permanent::new(TUNDRA),
            ],
        )
        .hand(P0, &[COUNTERSPELL, MANA_DRAIN, OUST, ISLAND])
        .library(P0, LANDS)
        .battlefield(
            P1,
            &[
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(SUBTLETY),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(THIEVING_SKYDIVER),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                Permanent::new(SEACHROME_COAST),
            ],
        )
        .hand(P1, &[STOCK_UP])
        .library(P1, LANDS)
        .actions(&[Action::cast(P1, STOCK_UP)]),
    // Teferi, Time Raveler's −3 announces how many targets, up to one, with an artifact and a
    // creature across the table; the human then announces one.
    Scenario::new("target-up-to-one")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(TEFERI_TIME_RAVELER), ISLAND_, PLAINS_])
        .library(P0, LANDS)
        .battlefield(
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
        .battlefield(P0, &[Permanent::new(JACE_VRYNS_PRODIGY), PLAINS_, ISLAND_])
        .hand(P0, &[OUST])
        .library(P0, LANDS)
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, OUST)]),
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
    // Get Lost destroys one of the human's Elite Spellbinders, leaving them two Maps; one
    // makes the other Spellbinder explore, revealing Oust.
    Scenario::new("choice-explore")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
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
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, GET_LOST),
            Action::activate(P0, MAP_TOKEN, 0),
        ]),
    // Jace, Vryn's Prodigy loots: it draws Stock Up, then asks for the discard.
    Scenario::new("choice-loot")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(JACE_VRYNS_PRODIGY), ISLAND_, PLAINS_])
        .hand(P0, &[COUNTERSPELL, OUST])
        .library(P0, &[STOCK_UP, ISLAND, PLAINS, ISLAND, PLAINS])
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::activate(P0, JACE_VRYNS_PRODIGY, 0)]),
    // Time Spiral, paid with six of the human's seven lands, untaps up to six lands, beside
    // the opponent's tapped and untapped ones.
    Scenario::new("choice-untap")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                ISLAND_, ISLAND_, ISLAND_, ISLAND_, PLAINS_, PLAINS_, PLAINS_,
            ],
        )
        .hand(P0, &[TIME_SPIRAL])
        .library(P0, LANDS)
        .battlefield(
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
    // Trinket Mage searches for up to one artifact of mana value 1 or less, of two.
    Scenario::new("choice-search")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[TRINKET_MAGE])
        .library(
            P0,
            &[MANA_CRYPT, ISLAND, CURRENCY_CONVERTER, PLAINS, ISLAND],
        )
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, TRINKET_MAGE),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
        ]),
    // Thirst for Discovery draws three, and the human discards one of two basic lands rather
    // than two cards.
    Scenario::new("choice-thirst")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THIRST_FOR_DISCOVERY, PLAINS, ISLAND, COUNTERSPELL])
        .library(P0, &[STOCK_UP, OUST, MANA_DRAIN, ISLAND, PLAINS])
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THIRST_FOR_DISCOVERY),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
        ]),
    // The human ends their turn with eight cards in hand, one over the maximum.
    Scenario::new("choice-cleanup")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[ISLAND_, PLAINS_])
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
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS),
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
            choose(P0, DecisionKind::Crew, SNAPCASTER_MAGE),
            choose(P0, DecisionKind::Crew, ELITE_SPELLBINDER),
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

/// A library to draw from for a few turns.
const LANDS: &[&str] = &[
    ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS,
];

const ISLAND_: Permanent = Permanent::new(ISLAND);
const PLAINS_: Permanent = Permanent::new(PLAINS);

/// The board named `name`.
pub fn named(name: &str) -> Result<&'static Scenario, String> {
    BOARDS
        .iter()
        .find(|b| b.name == name)
        .ok_or_else(|| format!("no board is named {name:?}"))
}

/// `player` plays the land `card`.
const fn play_land(player: Seat, card: &'static str) -> Action {
    Action::PlayLand { player, card }
}

/// `player` answers a decision of `kind` with the card `card`.
const fn choose(player: Seat, kind: DecisionKind, card: &'static str) -> Action {
    Action::choose(player, kind, card)
}

/// `player` casts `card` with its first casting option: Overlord of the Mistmoors' impending
/// cost, Thieving Skydiver's kicker, Damn's overload.
const fn cast_with_option(player: Seat, card: &'static str) -> Action {
    Action::Cast {
        player,
        card,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    }
}

/// How a board's scripts are going, shared by its two [`Scripted`] agents and the recording.
#[derive(Debug, Default)]
pub struct Progress {
    /// The first decision a script could not answer.
    pub failure: Option<String>,
    /// Each seat's next action, while any is left.
    pub next: [Option<Action>; 2],
}

pub type Shared = Arc<Mutex<Progress>>;

/// The progress of a board's scripts so far.
pub fn lock(progress: &Shared) -> std::sync::MutexGuard<'_, Progress> {
    progress.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An agent answering one seat's decisions from a board's actions for that seat, in order, by
/// the scenario vocabulary: each action answers the first decision it fits. Short of its next
/// action, the seat passes priority, pays by the first payment plan, and answers a question
/// with one answer; any other decision is the script's failure. Once its actions are taken,
/// it passes priority and takes the first candidate of anything else. Its view is the true
/// state: a script names the cards it means.
pub struct Scripted {
    table: GameTable,
    seat: Seat,
    script: VecDeque<Action>,
    progress: Shared,
}

impl Scripted {
    /// `seat`'s part of `board`'s actions, played on `table`. Every action must answer one
    /// decision.
    pub fn new(
        board: &Scenario,
        seat: Seat,
        table: &GameTable,
        progress: Shared,
    ) -> Result<Scripted, String> {
        let mut script = VecDeque::new();
        for action in board.actions {
            match *action {
                Action::PlayLand { player, .. }
                | Action::Cast { player, .. }
                | Action::Activate { player, .. }
                | Action::TurnFaceUp { player, .. }
                | Action::Answer { player, .. } => {
                    if player == seat {
                        script.push_back(*action);
                    }
                }
                _ => return Err(format!("{action:?}: a board's action answers one decision")),
            }
        }
        lock(&progress).next[seat.index()] = script.front().copied();
        Ok(Scripted {
            table: table.clone(),
            seat,
            script,
            progress,
        })
    }
}

impl Agent for Scripted {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        let s = obs.unmasked_root();
        let next = self.script.front().copied();
        if let Some(action) = next {
            let kind = match action {
                Action::Answer { kind, .. } => kind,
                _ => DecisionKind::Priority,
            };
            if kind == d.kind
                && let Some(c) = beryl_scenario::answer(&self.table, s, d, &action)
            {
                self.script.pop_front();
                lock(&self.progress).next[self.seat.index()] = self.script.front().copied();
                return c;
            }
        }
        if d.kind == DecisionKind::Priority
            && let Some(c) = beryl_scenario::answer(&self.table, s, d, &Action::pass(d.player))
        {
            return c;
        }
        if let Some(action) = next
            && d.kind != DecisionKind::PaymentPlan
            && d.len() > 1
        {
            lock(&self.progress).failure.get_or_insert_with(|| {
                format!(
                    "unscripted decision: {}, where the board's next action is {action:?}",
                    beryl_scenario::describe(&self.table, s, d),
                )
            });
        }
        Choice(0)
    }
}
