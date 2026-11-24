//! The sideboard cards that have entered this game.
//!
//! Implements: CR 400.11

use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, PerSeat};
use crate::state::{Object, State, Zone};
use crate::table::GameTable;

/// One bit for each occurrence in each player's original list. A card may later
/// exchange its slot with an indistinguishable card in a hidden zone, so the record
/// is independent of its pool slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Taken(PerSeat<u16>);

impl Taken {
    pub(crate) fn is_empty(self) -> bool {
        self == Self::default()
    }

    fn has(self, player: Seat, index: u8) -> bool {
        self.0[player.index()] & (1 << index) != 0
    }
}

impl<L: Layout> State<L> {
    /// The remaining sideboard occurrences and their game indices, in list order.
    ///
    /// Subgames currently offer no sideboard choices. A card already in a parent's
    /// library enters the subgame with that library (see [CR 730.2]).
    ///
    /// The list starts outside the game ([CR 400.11a]): "Cards in a player’s sideboard are
    /// outside the game."
    ///
    /// [CR 400.11a]: https://yawgatog.com/resources/magic-rules/#R40011a
    /// [CR 730.2]: https://yawgatog.com/resources/magic-rules/#R7302
    pub fn sideboard<'a>(
        &'a self,
        table: &'a GameTable,
        player: Seat,
    ) -> impl Iterator<Item = (u8, L::Card)> + 'a {
        self.sideboard_entries(table, player, false)
    }

    pub(crate) fn admitted<'a>(
        &'a self,
        table: &'a GameTable,
        player: Seat,
    ) -> impl Iterator<Item = (u8, L::Card)> + 'a {
        self.sideboard_entries(table, player, true)
    }

    fn sideboard_entries<'a>(
        &'a self,
        table: &'a GameTable,
        player: Seat,
        taken: bool,
    ) -> impl Iterator<Item = (u8, L::Card)> + 'a {
        let main_game = !self.in_subgame();
        table
            .sideboard(player)
            .zip(0u8..)
            .filter_map(move |(card, index)| {
                (main_game && self.sideboard_taken.has(player, index) == taken)
                    .then_some((index, card))
            })
    }

    /// Active cards in a player's deck slots or admitted from a sideboard into pool slots.
    /// A subgame also keeps the pool slots of cards that entered through the parent's library.
    pub(crate) fn in_game_card_slots(
        &self,
        table: &GameTable,
        player: Seat,
    ) -> impl Iterator<Item = L::Slot> + '_ {
        let subgame = self.in_subgame();
        let deck = (0..table.deck_len(player))
            .map(move |index| L::Slot::deck(player, index))
            .filter(move |&slot| !subgame || self.object(slot).zone != Zone::Nowhere);
        let pool = (subgame || table.sideboard::<L::Card>(player).len() != 0)
            .then_some(())
            .into_iter()
            .flat_map(|()| L::Slot::pool())
            .filter(move |&slot| self.is_pool_card(player, slot));
        deck.chain(pool)
    }

    fn is_pool_card(&self, player: Seat, slot: L::Slot) -> bool {
        let object = self.object(slot);
        object.zone != Zone::Nowhere && !object.is_token() && object.owner == player
    }

    /// Prepares a sideboard card in a free pool slot. The rules then reveal it, if required,
    /// and move it to its destination. The card is real and its owner keeps it through later
    /// zone changes.
    ///
    /// Admission lasts for the game ([CR 400.11b]): "Some effects bring cards into a game
    /// from outside the game. Those cards
    /// remain in the game until the game ends, their owner leaves the game, or a rule or effect
    /// removes them from the game, whichever comes first."
    ///
    /// [CR 400.11b]: https://yawgatog.com/resources/magic-rules/#R40011b
    ///
    /// # Errors
    ///
    /// Returns [`Full`] when no pool slot is free or its incarnation is exhausted.
    pub fn prepare_sideboard_card(
        &mut self,
        table: &GameTable,
        player: Seat,
        index: u8,
    ) -> Result<Option<L::Slot>, Full> {
        if self.in_subgame() {
            return Ok(None);
        }
        let Some(card) = table.sideboard_card(player, index) else {
            return Ok(None);
        };
        if self.sideboard_taken.has(player, index) {
            return Ok(None);
        }
        let slot = L::Slot::pool()
            .find(|&slot| {
                let object = self.object(slot);
                object.zone == Zone::Nowhere && object.incarnation < u16::MAX
            })
            .ok_or(Full)?;
        let incarnation = self.object(slot).incarnation;
        *self.object_mut(slot) = Object {
            card,
            owner: player,
            base_controller: player,
            incarnation,
            ..Object::EMPTY
        };
        self.sideboard_taken.0[player.index()] |= 1 << index;
        Ok(Some(slot))
    }

    pub(crate) fn check_sideboard(&self, table: &GameTable) -> Result<(), Violation> {
        if self.in_subgame() {
            return check(
                self.sideboard_taken.is_empty(),
                "a subgame admitted a sideboard card directly",
                None,
            );
        }
        for player in Seat::BOTH {
            let valid = (1 << table.sideboard::<L::Card>(player).len()) - 1;
            let taken = if valid == 0 {
                0
            } else {
                self.sideboard_taken.0[player.index()]
            };
            let at = Some(At::Player(player));
            check(taken & !valid == 0, "a missing sideboard occurrence", at)?;
            let in_game = L::Slot::pool()
                .filter(|&slot| self.is_pool_card(player, slot))
                .count();
            check(
                in_game == taken.count_ones() as usize,
                "pool cards disagree with sideboard admissions",
                at,
            )?;
        }
        Ok(())
    }
}
