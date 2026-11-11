//! `PlayerSet`: a set of players.

use crate::ids::Seat;

/// A set of players. Like a [`crate::Bitboard`] it is a set and has no order of its own:
/// the players act in turn order, which the state gives
/// ([`crate::State::players_in_turn_order`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerSet(u8);

impl PlayerSet {
    pub const NOBODY: PlayerSet = PlayerSet(0);
    pub const EVERYONE: PlayerSet = PlayerSet(0b11);

    #[inline]
    pub fn only(player: Seat) -> PlayerSet {
        PlayerSet(1 << player.get())
    }

    /// Every opponent of `you`
    /// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player
    /// game, a player's opponent is the other player."
    ///
    /// The one place "each opponent" meets the two-player game. With more players, "the
    /// player's opponents are all players not on their team"
    /// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): this function
    /// changes then, and no card does.
    #[inline]
    pub fn each_opponent(you: Seat) -> PlayerSet {
        PlayerSet::only(you.opponent())
    }

    #[inline]
    pub fn insert(&mut self, player: Seat) {
        self.0 |= 1 << player.get();
    }

    #[inline]
    pub fn contains(self, player: Seat) -> bool {
        self.0 & (1 << player.get()) != 0
    }

    /// The players in turn order from `active` (CR 101.4).
    #[inline]
    pub(crate) fn in_turn_order(self, active: Seat) -> impl Iterator<Item = Seat> {
        [active, active.opponent()]
            .into_iter()
            .filter(move |&player| self.contains(player))
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerSet;
    use crate::ids::Seat;

    #[test]
    fn players_act_in_turn_order_from_the_active_player() {
        let order = |who: PlayerSet, active| who.in_turn_order(active).collect::<Vec<_>>();
        assert_eq!(order(PlayerSet::EVERYONE, Seat::P1), [Seat::P1, Seat::P0]);
        assert_eq!(
            order(PlayerSet::each_opponent(Seat::P1), Seat::P1),
            [Seat::P0]
        );
        let mut who = PlayerSet::NOBODY;
        assert_eq!(order(who, Seat::P0), []);
        who.insert(Seat::P1);
        assert!(who.contains(Seat::P1) && !who.contains(Seat::P0));
        assert_eq!(who, PlayerSet::only(Seat::P1));
    }
}
