//! Rules that define a game's format. The rules engine applies them. A format adds a poison
//! loss threshold when the engine tracks poison.
//!
//! Implements: none. The format supplies the defaults used by the rules.

use beryl_card::TurnStep;

use crate::decision::Outcome;
use crate::player_set::PlayerSet;

/// Which rules a game starts with: two-player constructed ([`Format::CONSTRUCTED`]), or the
/// shared-deck format in which both players draw from one library and share one graveyard
/// ([`Format::SHARED_DECK`]).
///
/// One private byte. A getter whose answer is the same in both formats returns a constant
/// and never reads it, so the compiler still resolves those rules without loading data. No
/// hot path reads the byte: whether the zones are shared is copied into the state at the
/// deal, and every other difference is read at setup, at a mulligan or when each player
/// draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format(u8);

/// Whether each player has their own library and graveyard ([`Format::zones`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRule {
    /// Each player has their own library, hand and graveyard (see CR 400.1).
    PerPlayer,
    /// The players share one library and one graveyard, and each has their own hand.
    SharedLibraryAndGraveyard,
}

/// How cards are dealt when several players draw at the same time
/// ([`Format::simultaneous_draws`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimultaneousDraws {
    /// Each player draws from their own library (see CR 101.4).
    Together,
    /// From a shared library, one card at a time to each player in turn, starting with the
    /// active player.
    OneAtATimeFromActivePlayer,
}

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
    /// The London mulligan, and before a player's first regular mulligan a free one: a
    /// player whose hand has fewer than two land cards or fewer than two nonland cards may
    /// reveal it and draw seven again, putting nothing on the bottom for it.
    LondonWithFreeRedraw,
}

/// Who skips the draw on their first turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstDraw {
    SkippedByStartingPlayer,
}

/// How a card list is used when setting up the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckRole {
    Library,
    Sideboard,
    /// The first list is the one library both players draw from, and there is no other.
    SharedLibrary,
}

/// The format's state-based loss conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Losses {
    /// At or below this life total, the player loses.
    pub life: i16,
    /// Attempting to draw from an empty library causes a loss.
    pub empty_library: bool,
}

impl Format {
    pub const CONSTRUCTED: Self = Self(0);

    /// Both players draw from one shared library and share one graveyard, by the rules
    /// Wizards of the Coast published with the format's Secret Lair deck (magic.wizards.com,
    /// 2026-03-15): "Players have a shared library and graveyard."
    pub const SHARED_DECK: Self = Self(1);

    /// Whether the players share a library and a graveyard. See [CR 400.1]: "Each player has
    /// their own library, hand, and graveyard." The shared-deck format changes that: "Players
    /// have a shared library and graveyard. Anything referring to 'your' graveyard or library
    /// is referencing the shared zone."
    ///
    /// [CR 400.1]: https://yawgatog.com/resources/magic-rules/#R4001
    #[inline]
    pub const fn zones(self) -> ZoneRule {
        match self.0 {
            0 => ZoneRule::PerPlayer,
            _ => ZoneRule::SharedLibraryAndGraveyard,
        }
    }

    /// How cards are dealt when players draw at the same time. See [CR 101.4]: "If multiple
    /// players would make choices and/or take actions at the same time, the active player
    /// ... makes any choices required, then the next player in turn order". The shared-deck
    /// format: "Whenever multiple players are drawing cards simultaneously, they are dealt
    /// one at a time, starting with the active player".
    ///
    /// [CR 101.4]: https://yawgatog.com/resources/magic-rules/#R1014
    #[inline]
    pub const fn simultaneous_draws(self) -> SimultaneousDraws {
        match self.0 {
            0 => SimultaneousDraws::Together,
            _ => SimultaneousDraws::OneAtATimeFromActivePlayer,
        }
    }

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
    /// The shared-deck format adds: "When taking mulligans, if a player draws a hand with
    /// less than two lands or spells, that player may reveal the hand and take a free
    /// mulligan. This no longer applies once that player has taken a regular mulligan."
    ///
    /// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
    #[inline]
    pub const fn mulligan(self) -> Mulligan {
        match self.0 {
            0 => Mulligan::London,
            _ => Mulligan::LondonWithFreeRedraw,
        }
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
    #[inline]
    pub const fn turn(self) -> &'static [TurnStep] {
        &TurnStep::ALL
    }

    /// The deck supplies the library at setup. A constructed sideboard is a separate list
    /// (see [CR 100.4a]: "The four-card limit (see rule 100.2a) applies to the combined deck
    /// and sideboard.").
    ///
    /// The shared-deck format has one list, the shared library, and no sideboard.
    ///
    /// [CR 100.4a]: https://yawgatog.com/resources/magic-rules/#R1004a
    pub const fn decks(self) -> &'static [DeckRole] {
        match self.0 {
            0 => &[DeckRole::Library, DeckRole::Sideboard],
            _ => &[DeckRole::SharedLibrary],
        }
    }

    /// Upper bound on sideboard size across formats, for stack-allocated determinization buffers.
    pub const MAX_SIDEBOARD: usize = 15;

    /// See [CR 100.4a]: "In constructed play, a sideboard may contain no more than fifteen cards."
    ///
    /// The shared-deck format has no sideboard.
    ///
    /// [CR 100.4a]: https://yawgatog.com/resources/magic-rules/#R1004a
    pub const fn maximum_sideboard(self) -> usize {
        match self.0 {
            0 => Self::MAX_SIDEBOARD,
            _ => 0,
        }
    }

    /// See [CR 704.5a]: "If a player has 0 or less life, that player loses the game."
    /// See [CR 704.5b]: "If a player attempted to draw a card from a library with no cards
    /// in it since the last time state-based actions were checked, that player loses the game."
    ///
    /// [CR 704.5a]: https://yawgatog.com/resources/magic-rules/#R7045a
    /// [CR 704.5b]: https://yawgatog.com/resources/magic-rules/#R7045b
    #[inline]
    pub const fn losses(self) -> Losses {
        Losses {
            life: 0,
            empty_library: true,
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

#[cfg(test)]
mod tests;
