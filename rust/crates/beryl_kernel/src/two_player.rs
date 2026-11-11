//! The two-player table: the one module where "the other player" exists.
//!
//! The rules reach players through sets and orders ([`crate::State::players`],
//! [`crate::State::opponents_of`], [`crate::State::players_in_turn_order`],
//! [`crate::State::next_player`], [`crate::State::defending_players`]), which say what they
//! mean with any number of players. This module is the shortcut those replace: a seat has
//! exactly one other seat
//! ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player game, a
//! player's opponent is the other player."
//!
//! Nothing here is in scope until a module imports [`TwoPlayerSeat`], so the modules that
//! assume a table of two are the ones that name this one. Who may:
//!
//! - The kernel's own turn order ([`crate::State::next_player`]) and its test door
//!   ([`crate::board`]), which count turns by alternating.
//! - A caller outside the rules that means the other seat at this table: the session's
//!   protocol and saves (a view has a "you" and an "opponent"), search's evaluation of a
//!   position for one player against the other, the tools' fixtures, and tests.
//!
//! Rules code and card bodies never do: they ask the state the question the rule asks.
//!
//! A third player would break every caller, each in its own way: a view would have several
//! opponents to draw, an evaluation several positions to weigh, and a turn order more than
//! alternation ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031): "The game's
//! default turn order begins with the starting player and proceeds clockwise"). Then this
//! module is deleted, and the compiler lists them.

use crate::ids::Seat;

/// A seat at a table of two.
pub trait TwoPlayerSeat {
    /// The other seat of the two.
    #[must_use]
    fn opponent(self) -> Seat;
}

impl TwoPlayerSeat for Seat {
    #[inline]
    fn opponent(self) -> Seat {
        match self {
            Seat::P0 => Seat::P1,
            Seat::P1 => Seat::P0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TwoPlayerSeat;
    use crate::ids::Seat;

    #[test]
    fn seats_are_each_others_opponents() {
        assert_eq!(Seat::P0.opponent(), Seat::P1);
        assert_eq!(Seat::P1.opponent(), Seat::P0);
    }
}
