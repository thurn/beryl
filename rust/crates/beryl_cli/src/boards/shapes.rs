//! Boards for the prompt shapes on the stage (plan K26): each a shape's worked example, which
//! one fixture answers by clicks from the seat that is asked, and another watches from across
//! the table with the same play scripted.

use beryl_cards::names::{
    AANGS_ICEBERG, ABHORRENT_OCULUS, BEZA_THE_BOUNDING_SPRING, COUNTERSPELL, CURRENCY_CONVERTER,
    DAMN, ELITE_SPELLBINDER, ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY,
    JACE_WIELDER_OF_MYSTERIES, MALCOLM_ALLURING_SCOUNDREL, MANA_CRYPT, MANA_DRAIN, OUST, PLAINS,
    REPRIEVE, SINK_INTO_STUPOR, SNAPCASTER_MAGE, STOCK_UP, SWIFT_RECONFIGURATION,
    THE_WANDERING_EMPEROR, THIEVING_SKYDIVER, THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER,
    TRINKET_MAGE, TUNDRA,
};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, Seat, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{ISLAND_, LANDS, PLAINS_, cast_with_option, choose, play_land};

/// The permanents of [`pick`], by player.
const PICKERS: &[Permanent] = &[
    Permanent::new(TUNDRA),
    Permanent::new(THUNDERTRAP_TRAINER).sick(),
    Permanent::new(JACE_WIELDER_OF_MYSTERIES),
];
const WATCHERS: &[Permanent] = &[Permanent::new(ELITE_SPELLBINDER), ISLAND_];

/// Picking on the table: the human's main phase with a land to play, of two alike in hand;
/// a Tundra, whose two ways to tap open the choice among them; a Counterspell with nothing
/// to counter; a planeswalker whose ability targets a player; and an Oust, whose targets are
/// a creature of each player's, their own asked about before it is taken.
const fn pick(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[ISLAND, ISLAND, OUST, COUNTERSPELL])
        .battlefield(P0, PICKERS)
        .battlefield(P1, WATCHERS)
        .library(P0, LANDS)
        .library(P1, LANDS)
}

/// The permanents of [`zone`]: lands that pay for all its casts, with none to choose among.
const SEARCHERS: &[Permanent] = &[
    Permanent::new(JACE_VRYNS_PRODIGY),
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
];

/// The permanents of [`split`] and of [`order`].
const SPLITTERS: &[Permanent] = &[
    Permanent::new(AANGS_ICEBERG),
    Permanent::new(JACE_THE_MIND_SCULPTOR),
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
];
const ORDERERS: &[Permanent] = &[
    Permanent::new(CURRENCY_CONVERTER),
    ISLAND_,
    ISLAND_,
    ISLAND_,
];

/// Splitting cards into two piles: the human's main phase with Aang's Iceberg, whose scry of
/// two the board's script begins (its cost is paid by tapping lands, another shape's prompt);
/// a Stock Up to keep two of the top five with; and Jace, the Mind Sculptor, whose second
/// ability puts two cards of the hand back.
const fn split(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[STOCK_UP, COUNTERSPELL])
        .battlefield(P0, SPLITTERS)
        .battlefield(P1, WATCHERS)
        .library(
            P0,
            &[
                PLAINS,
                ISLAND,
                MANA_DRAIN,
                OUST,
                DAMN,
                REPRIEVE,
                TUNDRA,
                ISLAND,
                COUNTERSPELL,
                PLAINS,
                ISLAND,
            ],
        )
        .library(P1, LANDS)
}

/// Picking from a zone: the human's main phase with a Trinket Mage, which searches for one
/// of two artifacts; a Thundertrap Trainer, which looks at the top four for a spell; an
/// Abhorrent Oculus to exile six of a graveyard's seven for; and Jace, Vryn's Prodigy, which
/// draws a card and has one discarded.
const fn zone(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[TRINKET_MAGE, THUNDERTRAP_TRAINER, ABHORRENT_OCULUS])
        .battlefield(P0, SEARCHERS)
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
        .battlefield(P1, WATCHERS)
        .library(
            P0,
            &[
                PLAINS,
                MANA_CRYPT,
                ISLAND,
                CURRENCY_CONVERTER,
                PLAINS,
                DAMN,
                ISLAND,
                COUNTERSPELL,
                PLAINS,
                ISLAND,
            ],
        )
        .library(P1, LANDS)
}

/// Ordering triggers: the human's main phase with a Thirst for Discovery to discard two
/// cards to, and a Currency Converter, which triggers for each.
const fn order(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[THIRST_FOR_DISCOVERY, COUNTERSPELL, OUST])
        .battlefield(P0, ORDERERS)
        .battlefield(P1, WATCHERS)
        .library(P0, &[PLAINS, ISLAND, DAMN, REPRIEVE, MANA_DRAIN, ISLAND])
        .library(P1, LANDS)
}

/// The permanents of [`pay`]: three creatures of power 2, 2 and 3 to crew with, an Aang's
/// Iceberg to waterbend, and lands and a Mana Crypt that pay a cost in more ways than one.
const PAYERS: &[Permanent] = &[
    Permanent::new(JACE_VRYNS_PRODIGY),
    Permanent::new(SNAPCASTER_MAGE),
    Permanent::new(TRINKET_MAGE),
    Permanent::new(ELITE_SPELLBINDER),
    Permanent::new(AANGS_ICEBERG),
    Permanent::new(MANA_CRYPT),
    PLAINS_,
    PLAINS_,
    ISLAND_,
    ISLAND_,
];

/// Paying with permanents: the human's main phase with a Thieving Skydiver, whose mana the
/// lands and the Mana Crypt pay in several ways; a Swift Reconfiguration to make Jace, Vryn's
/// Prodigy a Vehicle that three creatures may crew; and Aang's Iceberg, whose waterbend
/// cost what is left untapped pays, permanents in place of mana among it.
const fn pay(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[THIEVING_SKYDIVER, SWIFT_RECONFIGURATION])
        .battlefield(P0, PAYERS)
        .battlefield(P1, WATCHERS)
        .library(P0, LANDS)
        .library(P1, LANDS)
}

/// The permanents of [`declare`], by player: the human's three creatures, and the opponent's
/// two with a planeswalker to attack beside its controller.
const DEFENDERS: &[Permanent] = &[
    Permanent::new(SNAPCASTER_MAGE),
    Permanent::new(ELITE_SPELLBINDER),
    Permanent::new(JACE_VRYNS_PRODIGY),
    PLAINS_,
    ISLAND_,
];
const ATTACKERS: &[Permanent] = &[
    Permanent::new(BEZA_THE_BOUNDING_SPRING),
    Permanent::new(TRINKET_MAGE),
    Permanent::new(THE_WANDERING_EMPEROR),
    PLAINS_,
    ISLAND_,
];

/// Declaring blockers and then attackers: the opponent's turn, in which its two creatures
/// attack and the human's three may block either, and the human's turn after it, with a
/// planeswalker across the table to aim an attacker at. It opens in `turn`, which is
/// `active`'s: the opponent's for the seat that blocks first, the human's for the seat that
/// watches.
const fn declare(name: &'static str, turn: u16, active: Seat) -> Scenario {
    Scenario::new(name)
        .at(turn, active, TurnStep::PrecombatMain)
        .hand(P0, &[COUNTERSPELL])
        .battlefield(P0, DEFENDERS)
        .battlefield(P1, ATTACKERS)
        .library(P0, LANDS)
        .library(P1, LANDS)
}

/// The opponent's attack that opens [`declare`]: the last creature that may attack ends
/// the declaration.
const ATTACK: &[Action] = &[
    choose(P1, DecisionKind::Attacker, BEZA_THE_BOUNDING_SPRING),
    choose(P1, DecisionKind::Attacker, TRINKET_MAGE),
];

/// The permanents of [`yes`]: a planeswalker that looks at the top of a library, a creature
/// that has a card discarded, and an artifact that asks about each card discarded.
const ASKERS: &[Permanent] = &[
    Permanent::new(JACE_THE_MIND_SCULPTOR),
    Permanent::new(JACE_VRYNS_PRODIGY),
    Permanent::new(CURRENCY_CONVERTER),
    ISLAND_,
];

/// Yes or no: the human's main phase with a Sink into Stupor to play as its land, which
/// asks for life to enter untapped; Jace, the Mind Sculptor, whose first ability looks at the
/// top card of the opponent's library and asks where it goes; and Jace, Vryn's Prodigy, whose
/// discard a Currency Converter asks about.
const fn yes(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[SINK_INTO_STUPOR, COUNTERSPELL])
        .battlefield(P0, ASKERS)
        .battlefield(P1, WATCHERS)
        .library(P0, LANDS)
        .library(P1, &[OUST, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS])
}

/// The opponent's permanents in [`number`], and in [`mode`], where a spell has two to
/// choose its target among.
const CRYPT: &[Permanent] = &[Permanent::new(MANA_CRYPT), ISLAND_];
const ARTIFACTS: &[Permanent] = &[
    Permanent::new(MANA_CRYPT),
    Permanent::new(CURRENCY_CONVERTER),
    ISLAND_,
];

/// A number: the human's main phase with a Thieving Skydiver, whose kicker is an X that four
/// lands pay up to two of, and a Mana Crypt across the table to take with it.
const fn number(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[THIEVING_SKYDIVER])
        .battlefield(P0, &[ISLAND_, ISLAND_, ISLAND_, ISLAND_])
        .battlefield(P1, CRYPT)
        .library(P0, LANDS)
        .library(P1, LANDS)
}

/// The permanents of [`mode`]: a Malcolm, Alluring Scoundrel one counter short of offering
/// the card it has discarded, and lands that cast a Sink into Stupor.
const OFFERERS: &[Permanent] = &[
    Permanent::new(MALCOLM_ALLURING_SCOUNDREL).counters(CounterKind::Chorus, 3),
    ISLAND_,
    ISLAND_,
    ISLAND_,
];

/// A mode: the human's main phase with a Sink into Stupor, a card to cast or to play as a
/// land, chosen between its two faces; and a Malcolm, Alluring Scoundrel to attack with,
/// which draws an Elite Spellbinder, has it discarded and offers it to cast.
const fn mode(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[SINK_INTO_STUPOR])
        .battlefield(P0, OFFERERS)
        .battlefield(P1, ARTIFACTS)
        .library(
            P0,
            &[ELITE_SPELLBINDER, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS],
        )
        .library(P1, LANDS)
}

/// The permanents of [`stand`], by player.
const OURS: &[Permanent] = &[Permanent::new(SNAPCASTER_MAGE)];
const THEIRS: &[Permanent] = &[
    Permanent::new(TRINKET_MAGE),
    Permanent::new(ELITE_SPELLBINDER),
];

/// A table for a prop to stand on (`fixture/prop.rs`): a creature of the human's and two of
/// the opponent's, the recipients of a division, and nothing else on the battlefield; and a
/// land in the human's hand, so that their main phase asks them something for the prop to
/// stand in place of.
const fn stand(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[ISLAND])
        .battlefield(P0, OURS)
        .battlefield(P1, THEIRS)
        .library(P0, LANDS)
        .library(P1, LANDS)
}

pub(super) const SHAPES: &[Scenario] = &[
    // The fixtures' clicks play these.
    yes("shape-yes"),
    number("shape-number"),
    mode("shape-mode"),
    // The same plays, in part, for the seat across the table to watch: a look at its top
    // card; a Thieving Skydiver kicked for one; and a Sink into Stupor played as a land.
    yes("shape-yes-theirs").actions(&[
        Action::activate(P0, JACE_THE_MIND_SCULPTOR, 0),
        Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
        Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
    ]),
    number("shape-number-theirs").actions(&[
        cast_with_option(P0, THIEVING_SKYDIVER),
        Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
    ]),
    mode("shape-mode-theirs").actions(&[
        play_land(P0, SINK_INTO_STUPOR),
        Action::answer(P0, DecisionKind::YesNo, Pick::No),
    ]),
    // The table the props stand on, from either seat.
    stand("shape-stand"),
    // The fixtures' clicks play these.
    pay("shape-pay"),
    declare("shape-declare", 4, P1).actions(ATTACK),
    split("shape-split").actions(&[Action::activate(P0, AANGS_ICEBERG, 1)]),
    zone("shape-zone"),
    order("shape-order"),
    // The same plays, for the seat across the table to watch: the scry, with one card put
    // on the bottom; the search, and a card drawn and discarded; and two cards discarded,
    // whose triggers their controller's seat orders.
    split("shape-split-theirs").actions(&[
        Action::activate(P0, AANGS_ICEBERG, 1),
        choose(P0, DecisionKind::ChooseUpTo, PLAINS),
        Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
    ]),
    zone("shape-zone-theirs").actions(&[
        Action::cast(P0, TRINKET_MAGE),
        Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
        choose(P0, DecisionKind::ChooseUpTo, MANA_CRYPT),
        Action::activate(P0, JACE_VRYNS_PRODIGY, 0),
        choose(P0, DecisionKind::ChooseObject, ABHORRENT_OCULUS),
    ]),
    order("shape-order-theirs").actions(&[
        Action::cast(P0, THIRST_FOR_DISCOVERY),
        Action::answer(P0, DecisionKind::YesNo, Pick::No),
        choose(P0, DecisionKind::ChooseObject, COUNTERSPELL),
        choose(P0, DecisionKind::ChooseObject, OUST),
    ]),
    // A Vehicle made and crewed, and a waterbend cost paid with a creature and two lands;
    // and an attack at a planeswalker and at its controller, whose blockers that player's
    // seat declares.
    pay("shape-pay-theirs").actions(&[
        Action::cast(P0, SWIFT_RECONFIGURATION),
        choose(P0, DecisionKind::Target, JACE_VRYNS_PRODIGY),
        Action::activate_granted(P0, JACE_VRYNS_PRODIGY, 0),
        choose(P0, DecisionKind::ChooseObject, SNAPCASTER_MAGE),
        choose(P0, DecisionKind::ChooseObject, ELITE_SPELLBINDER),
        Action::activate(P0, AANGS_ICEBERG, 1),
        choose(P0, DecisionKind::ManaPayment, TRINKET_MAGE),
        Action::answer(P0, DecisionKind::ManaPayment, Pick::Done),
        Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
    ]),
    declare("shape-declare-theirs", 3, P0).actions(&[
        choose(P0, DecisionKind::Attacker, ELITE_SPELLBINDER),
        choose(P0, DecisionKind::AttackTarget, THE_WANDERING_EMPEROR),
        choose(P0, DecisionKind::Attacker, SNAPCASTER_MAGE),
        Action::answer(P0, DecisionKind::AttackTarget, Pick::DefendingPlayer),
        Action::answer(P0, DecisionKind::Attacker, Pick::Done),
    ]),
    // The fixture's clicks play it.
    pick("shape-pick"),
    // The same play, for the seat across the table to watch.
    pick("shape-pick-theirs").actions(&[
        play_land(P0, ISLAND),
        Action::activate(P0, JACE_WIELDER_OF_MYSTERIES, 1),
        Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
        Action::cast(P0, OUST),
        choose(P0, DecisionKind::Target, ELITE_SPELLBINDER),
    ]),
];
