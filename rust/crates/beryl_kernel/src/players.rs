//! The players of a game, as the rules name them: sets and orders, never "the other player"
//! (the crate's contract, "Players"). Each answer is the two-player game's today, and each
//! function's rustdoc says what more players would change in it. A caller that asks one of
//! these changes with none of that.

use beryl_card::Zone;

use crate::bitboard::Bitboard;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Table};
use crate::player_set::PlayerSet;
use crate::state::{State, ZoneKey};
use crate::two_player::TwoPlayerSeat as _;

impl<L: Layout> State<L> {
    /// Every player ([CR 102.1](https://yawgatog.com/resources/magic-rules/#R1021)): "A
    /// player is one of the people in the game."
    ///
    /// With more players some leave before the game ends
    /// ([CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a): "A player who
    /// concedes leaves the game immediately"); this is then the players still in it.
    #[inline]
    pub fn players(&self) -> PlayerSet {
        PlayerSet::seats(L::Slot::PLAYERS)
    }

    /// The opponents of `player`
    /// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player
    /// game, a player's opponent is the other player."
    ///
    /// With more players and no teams it is every other player, as written here. Between
    /// teams, "the player's opponents are all players not on their team"
    /// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)).
    #[inline]
    pub fn opponents_of(&self, player: Seat) -> PlayerSet {
        self.players().without(player)
    }

    /// The player after `player` in turn order
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)): "The game's default
    /// turn order begins with the starting player and proceeds clockwise." It is who takes
    /// the next turn, and who receives priority when `player` passes it
    /// ([CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)): "Then the next
    /// player in turn order receives priority."
    ///
    /// At a table of two that is the other player. With more it is the next seat still in
    /// the game ([CR 800.4a](https://yawgatog.com/resources/magic-rules/#R8004a): "priority
    /// passes to the next player in turn order who's still in the game").
    #[inline]
    pub fn next_player(&self, player: Seat) -> Seat {
        debug_assert!(self.players().contains(player));
        player.opponent()
    }

    /// `players` in the order they act when several do
    /// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)): "If multiple players
    /// would make choices and/or take actions at the same time, the active player ... makes
    /// any choices required, then the next player in turn order ... makes any choices
    /// required, followed by the remaining nonactive players in turn order."
    #[inline]
    pub fn players_in_turn_order(
        &self,
        players: PlayerSet,
    ) -> impl Iterator<Item = Seat> + Clone + use<L> {
        self.players_in_turn_order_from(self.turn.active, players)
    }

    /// `players` in turn order from `first`: the order of what the rules have each player
    /// do in turn from a named one, as the start of the game does from the starting player
    /// ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "First, the starting
    /// player declares whether they will take a mulligan. Then each other player in turn
    /// order does the same."
    #[inline]
    pub fn players_in_turn_order_from(
        &self,
        first: Seat,
        players: PlayerSet,
    ) -> impl Iterator<Item = Seat> + Clone + use<L> {
        debug_assert!(self.players().contains(first));
        players.in_turn_order(first)
    }

    /// `players` by seat, the order a decision lists players in and a record keeps them in.
    ///
    /// It is no order of the rules, as [`State::in_candidate_order`] is none for objects: a
    /// `Choice` is an index into candidates listed this way, and the events of one
    /// simultaneous happening are written in it, so saves and recorded rollouts depend on
    /// it. Use it to list players, and otherwise only where the result is the same whatever
    /// order they come in, with a comment that says why. Players who act in turn take
    /// [`State::players_in_turn_order`].
    #[inline]
    pub fn players_in_seat_order(
        &self,
        players: PlayerSet,
    ) -> impl Iterator<Item = Seat> + Clone + use<L> {
        players.in_seat_order()
    }

    /// The defending players of this turn's combat
    /// ([CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062)): "During the combat
    /// phase of a two-player game, the nonactive player is the defending player; that
    /// player, planeswalkers they control, and battles they protect may be attacked."
    ///
    /// With more players it is the same set
    /// ([CR 802.2](https://yawgatog.com/resources/magic-rules/#R8022)): "all the attacking
    /// player's opponents are defending players during the combat phase."
    #[inline]
    pub fn defending_players(&self) -> PlayerSet {
        self.opponents_of(self.turn.active)
    }

    /// The defending player an attacking creature attacks now: the player, or the controller
    /// of the planeswalker ([CR 508.5](https://yawgatog.com/resources/magic-rules/#R5085)):
    /// "the defending player it's referring to is the player that creature is attacking, the
    /// controller of the planeswalker that creature is attacking, or the protector of the
    /// battle that creature is attacking." Nobody for a creature that is not attacking, and
    /// for one whose planeswalker was removed from combat
    /// ([CR 506.4c](https://yawgatog.com/resources/magic-rules/#R5064c)): "It continues to be
    /// an attacking creature, although it is not attacking any player, planeswalker, or
    /// battle."
    ///
    /// An attacker records the planeswalker it attacks and no player: at a table of two a
    /// creature that attacks a player attacks the one defending player. With more, the
    /// attack declaration records which
    /// ([CR 802.2a](https://yawgatog.com/resources/magic-rules/#R8022a): "one specific
    /// defending player, not to all of the defending players").
    #[inline]
    pub fn defending_player(&self, attacker: L::Slot) -> Option<Seat> {
        let o = self.object(attacker);
        if !o.is_attacking() {
            None
        } else if o.attacks_planeswalker() {
            o.combat.map(|planeswalker| self.controller(planeswalker))
        } else {
            self.defending_players().sole()
        }
    }

    /// The players who know the identity of the object in `slot` ([`State::knows`]).
    #[inline]
    pub fn who_knows(&self, slot: L::Slot) -> PlayerSet {
        self.players().filter(|player| self.knows(player, slot))
    }

    /// Every object in `zone`, whoever owns it: both libraries, say, or every card in exile
    /// ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)): "Each player has
    /// their own library, hand, and graveyard. The other zones are shared by all players."
    #[inline]
    pub fn in_zone(&self, zone: Zone) -> Bitboard<L::Slot> {
        match zone {
            Zone::Stack => self.zone(ZoneKey::Stack),
            Zone::Nowhere => Bitboard::EMPTY,
            _ => self
                .zones
                .members
                .iter()
                .fold(Bitboard::EMPTY, |all, owned| all | owned[zone as usize]),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::Seat;
    use crate::layout::Small;
    use crate::profile::Profile;
    use crate::state::State;

    #[test]
    fn the_rules_name_players_by_sets_and_orders() {
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.board().set_active(Seat::P1);
        let everyone = s.players();
        assert_eq!(everyone.count(), 2);
        assert!(everyone.contains(Seat::P0) && everyone.contains(Seat::P1));
        assert_eq!(s.opponents_of(Seat::P0).sole(), Some(Seat::P1));
        assert_eq!(s.defending_players(), s.opponents_of(Seat::P1));
        assert_eq!(s.next_player(Seat::P1), Seat::P0);

        let order = |players: &mut dyn Iterator<Item = Seat>| players.collect::<Vec<_>>();
        // The active player first (CR 101.4), or a named one; a decision lists them by seat.
        assert_eq!(
            order(&mut s.players_in_turn_order(everyone)),
            [Seat::P1, Seat::P0]
        );
        assert_eq!(
            order(&mut s.players_in_turn_order_from(Seat::P0, everyone)),
            [Seat::P0, Seat::P1]
        );
        assert_eq!(
            order(&mut s.players_in_seat_order(everyone)),
            [Seat::P0, Seat::P1]
        );
        assert_eq!(
            order(&mut s.players_in_turn_order(s.opponents_of(Seat::P1))),
            [Seat::P0]
        );
    }
}
