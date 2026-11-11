//! `PlayerSet`: a set of players.

use crate::ids::Seat;
use crate::two_player::TwoPlayerSeat as _;

/// A set of players. Like a [`crate::Bitboard`] it is a set and has no order of its own:
/// the players act in turn order, which the state gives
/// ([`crate::State::players_in_turn_order`]). The state also gives the sets the rules name:
/// every player ([`crate::State::players`]), a player's opponents
/// ([`crate::State::opponents_of`]) and the defending players
/// ([`crate::State::defending_players`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerSet(u8);

impl PlayerSet {
    pub const NOBODY: PlayerSet = PlayerSet(0);

    /// The players in the first `count` seats.
    #[inline]
    pub(crate) const fn seats(count: usize) -> PlayerSet {
        PlayerSet((1 << count) - 1)
    }

    #[inline]
    pub fn only(player: Seat) -> PlayerSet {
        PlayerSet(1 << player.get())
    }

    #[inline]
    pub fn insert(&mut self, player: Seat) {
        self.0 |= 1 << player.get();
    }

    #[inline]
    pub fn remove(&mut self, player: Seat) {
        self.0 &= !(1 << player.get());
    }

    /// The set without `player`.
    #[inline]
    #[must_use]
    pub fn without(mut self, player: Seat) -> PlayerSet {
        self.remove(player);
        self
    }

    #[inline]
    pub fn contains(self, player: Seat) -> bool {
        self.0 & (1 << player.get()) != 0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// Whether `f` holds for some member.
    #[inline]
    pub fn any(self, f: impl Fn(Seat) -> bool) -> bool {
        self.in_seat_order().any(f)
    }

    /// The members `f` holds for.
    #[inline]
    #[must_use]
    pub fn filter(self, f: impl Fn(Seat) -> bool) -> PlayerSet {
        let mut kept = PlayerSet::NOBODY;
        for player in self.in_seat_order().filter(|&player| f(player)) {
            kept.insert(player);
        }
        kept
    }

    /// The one member, of a set that has exactly one.
    #[inline]
    pub fn sole(self) -> Option<Seat> {
        if self.count() == 1 {
            Seat::try_new(self.0.trailing_zeros() as u8)
        } else {
            None
        }
    }

    /// The players in turn order from `first`
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)): "The game's default
    /// turn order begins with the starting player and proceeds clockwise." At a table of two
    /// that is `first`, then the other.
    #[inline]
    pub(crate) fn in_turn_order(self, first: Seat) -> impl Iterator<Item = Seat> + Clone {
        [first, first.opponent()]
            .into_iter()
            .filter(move |&player| self.contains(player))
    }

    /// The players by seat.
    #[inline]
    pub(crate) fn in_seat_order(self) -> impl Iterator<Item = Seat> + Clone {
        Seat::BOTH
            .into_iter()
            .filter(move |&player| self.contains(player))
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerSet;
    use crate::ids::Seat;

    #[test]
    fn players_act_in_turn_order_from_the_first() {
        let order = |who: PlayerSet, first| who.in_turn_order(first).collect::<Vec<_>>();
        let everyone = PlayerSet::seats(2);
        assert_eq!(order(everyone, Seat::P1), [Seat::P1, Seat::P0]);
        assert_eq!(order(everyone.without(Seat::P1), Seat::P1), [Seat::P0]);
        let mut who = PlayerSet::NOBODY;
        assert_eq!(order(who, Seat::P0), []);
        who.insert(Seat::P1);
        assert!(who.contains(Seat::P1) && !who.contains(Seat::P0));
        assert_eq!(who, PlayerSet::only(Seat::P1));
    }

    #[test]
    fn a_set_answers_without_an_order() {
        let everyone = PlayerSet::seats(2);
        assert_eq!(everyone.count(), 2);
        assert_eq!(everyone.sole(), None);
        assert_eq!(PlayerSet::NOBODY.sole(), None);
        assert!(PlayerSet::NOBODY.is_empty());
        assert_eq!(everyone.without(Seat::P0).sole(), Some(Seat::P1));
        assert!(everyone.any(|player| player == Seat::P1));
        assert_eq!(
            everyone.filter(|player| player == Seat::P0),
            PlayerSet::only(Seat::P0)
        );
        assert_eq!(
            everyone.in_seat_order().collect::<Vec<_>>(),
            [Seat::P0, Seat::P1]
        );
    }
}
