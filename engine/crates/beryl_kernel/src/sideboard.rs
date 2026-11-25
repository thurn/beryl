//! The sideboard cards that have entered this game, and each player's revealed companion.
//!
//! Implements: CR 400.11, CR 702.139b

use beryl_card::{CardDef, DeckCondition, Words};

use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, PerSeat};
use crate::state::{Object, State, Zone};
use crate::table::GameTable;

/// One bit for each occurrence in each player's original list. A card may later
/// exchange its slot with an indistinguishable card in a hidden zone, so the record
/// is independent of its pool slot. Beside them, each player's revealed companion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Taken {
    taken: PerSeat<u16>,
    /// One more than the list position of the companion each player revealed before the
    /// game began, or 0 for none. Unlike the admissions, nothing else in the state records
    /// it, so the state's hash includes it once a player has revealed one.
    companion: PerSeat<u8>,
}

impl Taken {
    pub(crate) fn is_empty(self) -> bool {
        self.taken == PerSeat::default()
    }

    /// Whether a player has revealed a companion.
    pub(crate) fn has_companion(self) -> bool {
        self.companion != PerSeat::default()
    }

    /// The companions the players revealed, for the state's hash.
    pub(crate) fn companions(self) -> PerSeat<u8> {
        self.companion
    }

    fn has(self, player: Seat, index: u8) -> bool {
        self.taken[player.index()] & (1 << index) != 0
    }
}

/// Whether a deck fulfills a companion ability's condition: every card of the deck that
/// `each` accepts passes `has`, read as cards outside the game are, with X as 0.
///
/// The deck is the starting deck ([CR 702.139b]): "If a companion ability refers to your
/// starting deck, it refers to your deck after you’ve set aside any sideboard cards."
///
/// [CR 702.139b]: https://yawgatog.com/resources/magic-rules/#R702139b
fn fulfills<'a>(
    condition: &DeckCondition,
    deck: impl IntoIterator<Item = &'a CardDef>,
    owner: Seat,
) -> bool {
    let reads = |filter, card| crate::filter::matches_outside(filter, card, owner, &Words::NONE, 0);
    deck.into_iter()
        .all(|card| !reads(&condition.each, card) || reads(&condition.has, card))
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

    /// The sideboard cards `player` may reveal as their companion before the game begins:
    /// each card with a companion ability whose condition their deck fulfills, by its first
    /// occurrence in the list, each card once.
    ///
    /// See [CR 103.2b]: "If any players wish to reveal a card with a companion ability that
    /// they own from outside the game, they may do so. A player may reveal no more than one
    /// card this way, and they may do so only if their deck fulfills the condition of that
    /// card’s companion ability."
    ///
    /// [CR 103.2b]: https://yawgatog.com/resources/magic-rules/#R1032b
    pub fn companion_choices<'a>(
        &'a self,
        table: &'a GameTable,
        player: Seat,
    ) -> impl Iterator<Item = (u8, L::Card)> + 'a {
        let deck = move || table.deck::<L::Card>(player).map(|card| table.def(card));
        self.sideboard(table, player).filter(move |&(index, card)| {
            let first = table
                .sideboard::<L::Card>(player)
                .position(|other| other == card);
            first == Some(usize::from(index))
                && table.def(card).front().abilities.iter().any(|ability| {
                    ability
                        .companion_condition()
                        .is_some_and(|condition| fulfills(condition, deck(), player))
                })
        })
    }

    /// Records the sideboard occurrence at `index` as `player`'s revealed companion
    /// ([`State::companion_choices`]).
    pub fn reveal_companion(&mut self, player: Seat, index: u8) {
        debug_assert_eq!(self.sideboard_taken.companion[player.index()], 0);
        self.sideboard_taken.companion[player.index()] = index + 1;
    }

    /// The list position and card of `player`'s revealed companion while it is still outside
    /// the game. Once it has entered the game, by the companion's special action or by any
    /// other effect, it is no longer outside the game and this is none.
    pub fn companion(&self, table: &GameTable, player: Seat) -> Option<(u8, L::Card)> {
        let index = self.sideboard_taken.companion[player.index()].checked_sub(1)?;
        if self.in_subgame() || self.sideboard_taken.has(player, index) {
            return None;
        }
        table
            .sideboard_card(player, index)
            .map(|card| (index, card))
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
        self.sideboard_taken.taken[player.index()] |= 1 << index;
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
                self.sideboard_taken.taken[player.index()]
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
