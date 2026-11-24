//! Rules that define a game's format. The rules engine applies them.
//!
//! Implements: none. The format supplies the defaults used by the rules.

use beryl_card::{TurnStep, Zone};

use crate::decision::Outcome;
use crate::player_set::PlayerSet;

/// Which rules a game starts with. The sole supported format is two-player constructed.
/// Keeping the format zero-sized lets the compiler resolve its rules without loading data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format(());

/// How seats are grouped into sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Teams {
    None,
}

/// What the winner of the starting-player draw chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingPlayer {
    PlayOrDraw,
}

/// How players replace their opening hands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mulligan {
    London,
}

/// Who skips the draw on their first turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstDraw {
    SkippedByStartingPlayer,
}

/// Whether each player has their own zone or the entire game shares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRule {
    PerPlayer(Zone),
    Shared(Zone),
}

/// How a card list is used when setting up the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckRole {
    Library,
    Sideboard,
}

/// The format's state-based loss conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Losses {
    /// At or below this life total, the player loses.
    pub life: i16,
    /// Attempting to draw from an empty library causes a loss.
    pub empty_library: bool,
    /// At or above this number of poison counters, the player loses.
    /// The engine does not track poison yet.
    pub poison: u8,
}

impl Format {
    pub const CONSTRUCTED: Self = Self(());

    #[inline]
    pub const fn seats(self) -> u8 {
        2
    }

    pub const fn teams(self) -> Teams {
        Teams::None
    }

    /// See [CR 103.4]: "Each player begins the game with a starting life total of 20."
    ///
    /// [CR 103.4]: https://yawgatog.com/resources/magic-rules/#R1034
    #[inline]
    pub const fn starting_life(self) -> i16 {
        20
    }

    /// See [CR 103.5]: "Each player draws a number of cards equal to their starting hand
    /// size, which is normally seven."
    ///
    /// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
    #[inline]
    pub const fn opening_hand(self) -> u8 {
        7
    }

    /// The hand size to reduce to during cleanup (see [CR 514.1]), "normally seven".
    ///
    /// [CR 514.1]: https://yawgatog.com/resources/magic-rules/#R5141
    #[inline]
    pub const fn maximum_hand(self) -> u32 {
        7
    }

    /// See [CR 305.2]: "A player can normally play one land during their turn".
    ///
    /// [CR 305.2]: https://yawgatog.com/resources/magic-rules/#R3052
    #[inline]
    pub const fn lands_each_turn(self) -> u8 {
        1
    }

    /// See [CR 103.1]: "At the start of a game, the players determine which one of them
    /// will choose who takes the first turn."
    ///
    /// [CR 103.1]: https://yawgatog.com/resources/magic-rules/#R1031
    #[inline]
    pub const fn starting_player(self) -> StartingPlayer {
        StartingPlayer::PlayOrDraw
    }

    /// See [CR 103.5]: "To take a mulligan, a player shuffles the cards in their hand back
    /// into their library, draws a new hand of cards equal to their starting hand size".
    ///
    /// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
    #[inline]
    pub const fn mulligan(self) -> Mulligan {
        Mulligan::London
    }

    /// See [CR 103.8a]: "the player who plays first skips the draw step".
    ///
    /// [CR 103.8a]: https://yawgatog.com/resources/magic-rules/#R1038a
    #[inline]
    pub const fn first_draw(self) -> FirstDraw {
        FirstDraw::SkippedByStartingPlayer
    }

    /// Steps in order, including the optional first-strike damage step
    /// (see [CR 500.1]: "The beginning, combat, and ending phases are further broken down
    /// into steps, which proceed in order.").
    ///
    /// [CR 500.1]: https://yawgatog.com/resources/magic-rules/#R5001
    pub const fn turn(self) -> &'static [TurnStep] {
        &TurnStep::ALL
    }

    /// The engine's card zones (see [CR 400.1]: "Each player has their own library, hand,
    /// and graveyard. The other zones are shared by all players."). The command zone's
    /// emblems have their own storage.
    ///
    /// [CR 400.1]: https://yawgatog.com/resources/magic-rules/#R4001
    pub const fn zones(self) -> &'static [ZoneRule] {
        &[
            ZoneRule::PerPlayer(Zone::Library),
            ZoneRule::PerPlayer(Zone::Hand),
            ZoneRule::PerPlayer(Zone::Graveyard),
            ZoneRule::Shared(Zone::Battlefield),
            ZoneRule::Shared(Zone::Exile),
            ZoneRule::Shared(Zone::Stack),
        ]
    }

    /// The deck supplies the library at setup. A constructed sideboard is a separate list
    /// (see [CR 100.4a]: "The four-card limit (see rule 100.2a) applies to the combined deck
    /// and sideboard.").
    ///
    /// [CR 100.4a]: https://yawgatog.com/resources/magic-rules/#R1004a
    pub const fn decks(self) -> &'static [DeckRole] {
        &[DeckRole::Library, DeckRole::Sideboard]
    }

    /// See [CR 100.4a]: "In constructed play, a sideboard may contain no more than fifteen cards."
    ///
    /// [CR 100.4a]: https://yawgatog.com/resources/magic-rules/#R1004a
    pub const fn maximum_sideboard(self) -> usize {
        15
    }

    /// See [CR 704.5a]: "If a player has 0 or less life, that player loses the game."
    /// See [CR 704.5b]: "If a player attempted to draw a card from a library with no cards
    /// in it since the last time state-based actions were checked, that player loses the game."
    /// See [CR 704.5c]: "If a player has ten or more poison counters, that player loses the game."
    ///
    /// [CR 704.5a]: https://yawgatog.com/resources/magic-rules/#R7045a
    /// [CR 704.5b]: https://yawgatog.com/resources/magic-rules/#R7045b
    /// [CR 704.5c]: https://yawgatog.com/resources/magic-rules/#R7045c
    #[inline]
    pub const fn losses(self) -> Losses {
        Losses {
            life: 0,
            empty_library: true,
            poison: 10,
        }
    }

    /// The result after a loss if at most one side is left. Without teams, each seat is
    /// its own side (see [CR 104.2a]: "A player still in the game wins the game if that
    /// player's opponents have all left the game."). No remaining side means a draw
    /// (see [CR 104.4a]: "If all the players remaining in a game lose simultaneously, the
    /// game is a draw.").
    ///
    /// [CR 104.2a]: https://yawgatog.com/resources/magic-rules/#R1042a
    /// [CR 104.4a]: https://yawgatog.com/resources/magic-rules/#R1044a
    #[inline]
    pub fn outcome_after_loss(self, remaining: PlayerSet) -> Option<Outcome> {
        debug_assert!(remaining.count() < u32::from(self.seats()));
        match self.teams() {
            Teams::None => {
                if self.seats() > 2 && remaining.count() > 1 {
                    return None;
                }
                Some(remaining.sole().map_or(Outcome::Draw, Outcome::Win))
            }
        }
    }
}
