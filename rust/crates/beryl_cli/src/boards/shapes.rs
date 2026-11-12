//! Boards for the prompt shapes on the stage (plan K26): each a shape's worked example, which
//! one fixture answers by clicks from the seat that is asked, and another watches from across
//! the table with the same play scripted.

use beryl_cards::names::{
    AANGS_ICEBERG, ABHORRENT_OCULUS, COUNTERSPELL, CURRENCY_CONVERTER, DAMN, ELITE_SPELLBINDER,
    ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY, JACE_WIELDER_OF_MYSTERIES, MANA_CRYPT,
    MANA_DRAIN, OUST, PLAINS, REPRIEVE, STOCK_UP, THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER,
    TRINKET_MAGE, TUNDRA,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{ISLAND_, LANDS, choose, play_land};

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

pub(super) const SHAPES: &[Scenario] = &[
    // The fixtures' clicks play these.
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
