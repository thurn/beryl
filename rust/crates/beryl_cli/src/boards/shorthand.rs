//! The names and constructors the boards share.

use beryl_cards::names::{ISLAND, PLAINS};
use beryl_engine::card::{Castings, FaceIx};
use beryl_engine::{DecisionKind, Seat};
use beryl_scenario::{Action, Permanent};

/// The tokens the boards name, as the cards that make them name them.
pub(super) const INSECT_TOKEN: &str = "Insect Token";
pub(super) const MAP_TOKEN: &str = "Map Token";
pub(super) const FISH_TOKEN: &str = "Fish Token";
pub(super) const SAMURAI_TOKEN: &str = "Samurai Token";
pub(super) const TREASURE_TOKEN: &str = "Treasure Token";
/// Jace, Vryn's Prodigy's back face.
pub(super) const JACE_TELEPATH_UNBOUND: &str = "Jace, Telepath Unbound";

/// A library to draw from for a few turns.
pub(super) const LANDS: &[&str] = &[
    ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS,
];

pub(super) const ISLAND_: Permanent = Permanent::new(ISLAND);
pub(super) const PLAINS_: Permanent = Permanent::new(PLAINS);

/// `player` plays the land `card`.
pub(super) const fn play_land(player: Seat, card: &'static str) -> Action {
    Action::PlayLand { player, card }
}

/// `player` answers a decision of `kind` with the card `card`.
pub(super) const fn choose(player: Seat, kind: DecisionKind, card: &'static str) -> Action {
    Action::choose(player, kind, card)
}

/// `player` casts `card` with its first casting option: Overlord of the Mistmoors' impending
/// cost, Thieving Skydiver's kicker, Damn's overload, Thundertrap Trainer's offspring.
pub(super) const fn cast_with_option(player: Seat, card: &'static str) -> Action {
    Action::Cast {
        player,
        card,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    }
}
