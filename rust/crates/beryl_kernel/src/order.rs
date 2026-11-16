//! The orders a set of objects or of players is read in, each asked of the state by name
//! (the crate's contract, "Sets and orders"). A [`Bitboard`] has none of its own: the
//! battlefield, a library and a graveyard keep theirs ([`State::battlefield`],
//! [`State::library_from_top`], [`State::graveyard`]), and the ones here put a set in order.

use beryl_card::Zone;

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{State, ZoneKey};
use crate::table::GameTable;

impl<L: Layout> State<L> {
    /// A player's library, top card first
    /// ([CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012)): "Each library must
    /// be kept in a single face-down pile."
    #[inline]
    pub fn library_from_top(
        &self,
        player: Seat,
    ) -> impl DoubleEndedIterator<Item = L::Slot> + ExactSizeIterator + '_ {
        self.library(player).iter().rev().copied()
    }

    /// The cards in a hand by definition key. Each card belongs to the owner of the hand,
    /// so a queue with room for their deck holds them all. [CR 400.3]: "If an object would
    /// go to any library, graveyard, or hand other than its owner's, it goes to its owner's
    /// corresponding zone."
    ///
    /// [CR 400.3]: https://yawgatog.com/resources/magic-rules/#R4003
    pub fn hand_by_definition(&self, player: Seat) -> L::Queue<L::Slot, 64> {
        let mut hand: L::Queue<L::Slot, 64> = Default::default();
        for slot in self.zone(ZoneKey::Of(player, Zone::Hand)).iter() {
            if self.card_behind(slot).is_some() {
                hand.push_bounded(slot); // One player's deck in either layout.
            }
        }
        hand.as_mut_slice()
            .sort_by_key(|&slot| self.definition(slot).key());
        hand
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
        if !(rest & self.in_zone(Zone::Battlefield)).is_empty() {
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

    /// The sources of a mana payment, by definition. Within each definition the payment
    /// keeps the slot order used by recorded games and rollout fingerprints. [CR 601.2g]
    /// says "If the total cost includes a mana payment, the player then has a chance
    /// to activate mana abilities."
    ///
    /// [CR 601.2g]: https://yawgatog.com/resources/magic-rules/#R6012g
    pub fn mana_payment_sources(
        &self,
        members: Bitboard<L::Slot>,
        count: u8,
    ) -> Result<L::Queue<L::Slot, 64>, Full> {
        let mut sources: L::Queue<L::Slot, 64> = Default::default();
        let key = |slot| self.definition(slot).key();
        let first = members.iter().next().map(key);
        let count = usize::from(count);
        if members.all(|slot| Some(key(slot)) == first) {
            for slot in members.iter().take(count) {
                sources.push(slot)?;
            }
            return Ok(sources);
        }
        for slot in members.iter() {
            sources.push(slot)?;
        }
        sources.as_mut_slice().sort_by_key(|&slot| key(slot));
        while sources.len() > count {
            sources.pop();
        }
        Ok(sources)
    }
}
