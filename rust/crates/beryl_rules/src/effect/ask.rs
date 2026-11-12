//! How a card body asks its player something (architecture §7.3). The questions are the
//! [`Resolution`](crate::resolution::Resolution)'s; these are the shim's ways to them, and to
//! the discards that ask ([`discard`]).

use super::EffectCtx;
use crate::actions::discard;
use crate::bitboard::Bitboard;
use crate::card::{Prompt, Stop};
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

    /// [`discard::discard_one_of`] the cards in `player`'s hand.
    pub fn discard_a_card(&mut self, player: Seat) -> Result<Option<ObjectRef<L>>, Stop> {
        let hand = self.game.state.zone(ZoneKey::Of(player, Zone::Hand));
        discard::discard_one_of(&mut self.game, &mut self.resolution, player, hand)
    }

    /// [`discard::discard_cards`].
    pub fn discard_cards(&mut self, player: Seat, n: u8) -> Result<Bitboard<L::Slot>, Stop> {
        discard::discard_cards(&mut self.game, &mut self.resolution, player, n)
    }

    /// [`discard::discard_one_of`].
    pub fn discard_one_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
    ) -> Result<Option<ObjectRef<L>>, Stop> {
        discard::discard_one_of(&mut self.game, &mut self.resolution, player, cards)
    }
}
