//! Boards for the prompt shapes on the stage (plan K26): each a shape's worked example, which
//! one fixture answers by clicks from the seat that is asked, and another watches from across
//! the table with the same play scripted.

use beryl_cards::names::{
    COUNTERSPELL, ELITE_SPELLBINDER, ISLAND, OUST, THUNDERTRAP_TRAINER, TUNDRA,
};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};

use super::shorthand::{ISLAND_, LANDS, choose, play_land};

/// The permanents of [`pick`], by player.
const PICKERS: &[Permanent] = &[
    Permanent::new(TUNDRA),
    Permanent::new(THUNDERTRAP_TRAINER).sick(),
];
const WATCHERS: &[Permanent] = &[Permanent::new(ELITE_SPELLBINDER), ISLAND_];

/// Picking on the table: the human's main phase with a land to play, of two alike in hand;
/// a Tundra, whose two ways to tap open the choice among them; a Counterspell with nothing
/// to counter; and an Oust, whose targets are a creature of each player's, their own asked
/// about before it is taken.
const fn pick(name: &'static str) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .hand(P0, &[ISLAND, ISLAND, OUST, COUNTERSPELL])
        .battlefield(P0, PICKERS)
        .battlefield(P1, WATCHERS)
        .library(P0, LANDS)
        .library(P1, LANDS)
}

pub(super) const SHAPES: &[Scenario] = &[
    // The fixture's clicks play it.
    pick("shape-pick"),
    // The same play, for the seat across the table to watch.
    pick("shape-pick-theirs").actions(&[
        play_land(P0, ISLAND),
        Action::cast(P0, OUST),
        choose(P0, DecisionKind::Target, ELITE_SPELLBINDER),
    ]),
];
