//! How a card body asks its player something (architecture §7.3). The questions are the
//! [`Resolution`](crate::resolution::Resolution)'s; these are the shim's ways to them, and the
//! discards that ask.

use super::EffectCtx;
use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::decision::{DecisionKind, Opt};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{ObjectRef, Zone, ZoneKey};

impl<L: Layout> EffectCtx<'_, L> {
    pub(super) fn ask_options(
        &mut self,
        player: Seat,
        kind: DecisionKind,
        opts: &[Opt<L>],
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<Opt<L>, Stop> {
        (self.resolution).ask_options(&mut self.game, player, kind, opts, prompt, about)
    }

    /// [`Resolution::choose_yes_no`](crate::resolution::Resolution::choose_yes_no).
    pub fn choose_yes_no(
        &mut self,
        player: Seat,
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<bool, Stop> {
        (self.resolution).choose_yes_no(&mut self.game, player, prompt, about)
    }

    /// [`Resolution::choose_up_to`](crate::resolution::Resolution::choose_up_to).
    pub fn choose_up_to(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        n: u8,
        prompt: Prompt,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        (self.resolution).choose_up_to(&mut self.game, player, candidates, n, prompt)
    }

    /// [`Resolution::choose_object`](crate::resolution::Resolution::choose_object).
    pub fn choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<L::Slot, Stop> {
        (self.resolution).choose_object(&mut self.game, player, candidates, prompt)
    }

    /// [`Resolution::may_choose_object`](crate::resolution::Resolution::may_choose_object).
    pub fn may_choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<Option<L::Slot>, Stop> {
        (self.resolution).may_choose_object(&mut self.game, player, candidates, prompt)
    }

    /// `player` discards a card of their choice
    /// ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)): "By default, effects
    /// that cause a player to discard a card allow the affected player to choose which card
    /// to discard." Returns the card in the graveyard, or `None` from an empty hand.
    pub fn discard_a_card(&mut self, player: Seat) -> Result<Option<ObjectRef<L>>, Stop> {
        self.discard_one_of(
            player,
            self.game.state.zone(ZoneKey::Of(player, Zone::Hand)),
        )
    }

    /// `player` discards `n` cards of their choice, or their whole hand if it has fewer
    /// (CR 701.9b), one pick for each: the picks of one prompt, which says at each how many
    /// are still to discard. Returns the cards, in the graveyard.
    pub fn discard_cards(&mut self, player: Seat, n: u8) -> Result<Bitboard<L::Slot>, Stop> {
        let mut discarded = Bitboard::EMPTY;
        for left in (1..=n).rev() {
            let hand = self.game.state.zone(ZoneKey::Of(player, Zone::Hand));
            let Some(card) = self.discard_of(player, hand, left)? else {
                break;
            };
            discarded.insert(card.slot);
        }
        Ok(discarded)
    }

    /// `player` discards one of `cards`, cards in their hand, of their choice, as Thirst for
    /// Discovery's "discard a basic land card" (CR 701.9b). Returns the card in the
    /// graveyard, or `None` when there is none to discard.
    pub fn discard_one_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
    ) -> Result<Option<ObjectRef<L>>, Stop> {
        self.discard_of(player, cards, 1)
    }

    /// One discard of `left` still to make, among `cards`.
    fn discard_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
        left: u8,
    ) -> Result<Option<ObjectRef<L>>, Stop> {
        debug_assert!((cards & !self.game.state.zone(ZoneKey::Of(player, Zone::Hand))).is_empty());
        if cards.is_empty() {
            return Ok(None);
        }
        let prompt = Prompt::new(Purpose::Discard).most(left);
        let kind = DecisionKind::ChooseObject;
        let none = Bitboard::EMPTY;
        let slot =
            (self.resolution).ask_object(&mut self.game, player, kind, cards, prompt, none)?;
        self.discard(self.game.state.object_ref(slot));
        Ok(Some(self.game.state.object_ref(slot)))
    }
}
