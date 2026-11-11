//! The orders a set of objects or of players is read in, each asked of the state by name
//! (the crate's contract, "Sets and orders"). A [`Bitboard`] has none of its own: the
//! battlefield, a library and a graveyard keep theirs ([`State::battlefield`],
//! [`State::library_from_top`], [`State::graveyard`]), and the ones here put a set in order.

use beryl_card::Zone;

use crate::bitboard::{Bitboard, Slots};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::player_set::PlayerSet;
use crate::state::{State, ZoneKey};
use crate::table::GameTable;

impl<L: Layout> State<L> {
    /// `objects` by ascending slot, the order a decision lists its candidates in.
    ///
    /// It is no order of the rules: the deal and every shuffle put cards in slots at random
    /// (architecture §14). A `Choice` is an index into candidates listed this way, so saves
    /// and recorded rollouts depend on it. Use it to list candidates, and otherwise only
    /// where the result is the same whatever order the members come in, with a comment that
    /// says why.
    #[inline]
    pub fn in_candidate_order(&self, objects: Bitboard<L::Slot>) -> Slots<L::Slot> {
        objects.iter()
    }

    /// A player's library, top card first
    /// ([CR 401.1](https://yawgatog.com/resources/magic-rules/#R4011)): "Each player's
    /// library is kept in a single face-down pile."
    #[inline]
    pub fn library_from_top(
        &self,
        player: Seat,
    ) -> impl DoubleEndedIterator<Item = L::Slot> + ExactSizeIterator + '_ {
        self.library(player).iter().rev().copied()
    }

    /// `players` in the order they act when several do
    /// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)): "If multiple players
    /// would make choices and/or take actions at the same time, the active player makes any
    /// choices required, then the next player in turn order".
    #[inline]
    pub fn players_in_turn_order(&self, players: PlayerSet) -> impl Iterator<Item = Seat> + use<L> {
        players.in_turn_order(self.turn.active)
    }

    /// `objects` in an order that depends on the game alone: permanents in the order they
    /// entered the battlefield, then each library's cards top first, then the rest by name.
    /// Slots are no order at all: the deal and shuffles put cards in them at random
    /// (architecture §14). A batch moves in this order, so cards put into a graveyard
    /// together lie in it in an order no seed decides, which a cost that counts from the
    /// bottom of a graveyard sees
    /// ([CR 404.2](https://yawgatog.com/resources/magic-rules/#R4042)): "Each graveyard is kept in
    /// a single face-up pile. A player can examine the cards in any graveyard at any time but
    /// normally can't change their order."
    pub fn in_game_order(
        &self,
        t: &GameTable,
        objects: Bitboard<L::Slot>,
    ) -> Result<L::Queue<L::Slot, 128>, Full> {
        let mut ordered: L::Queue<L::Slot, 128> = Default::default();
        if objects.count() < 2 {
            for slot in objects.iter() {
                ordered.push(slot)?;
            }
            return Ok(ordered);
        }
        let mut rest = objects;
        let zone =
            |z: Zone| self.zone(ZoneKey::Of(Seat::P0, z)) | self.zone(ZoneKey::Of(Seat::P1, z));
        if !(rest & zone(Zone::Battlefield)).is_empty() {
            for &slot in self.battlefield() {
                if rest.contains(slot) {
                    rest.remove(slot);
                    ordered.push(slot)?;
                }
            }
        }
        for player in Seat::BOTH {
            if (rest & self.zone(ZoneKey::Of(player, Zone::Library))).is_empty() {
                continue;
            }
            for slot in self.library_from_top(player) {
                if rest.contains(slot) {
                    rest.remove(slot);
                    ordered.push(slot)?;
                }
            }
        }
        let first = ordered.as_slice().len();
        for slot in rest.iter() {
            ordered.push(slot)?;
        }
        // Copies of a card alike but for their slots are interchangeable wherever they go.
        if ordered.as_slice().len() - first > 1 {
            ordered.as_mut_slice()[first..].sort_by_key(|&slot| self.definition_name(t, slot));
        }
        Ok(ordered)
    }
}
