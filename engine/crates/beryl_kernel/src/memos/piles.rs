//! The cards of merged and melded permanents in the memo store.
//! Implements: none. Finds the cards a permanent made of several cards became.

use super::{Kinds, Layout, Memo, State, Zone};
use crate::bitboard::Bitboard;

impl<L: Layout> State<L> {
    /// Whether any permanent is made of several cards now, a merged or melded one with a
    /// card under it ([`Memo::PartOf`]): one AND.
    #[inline]
    pub fn has_piles(&self) -> bool {
        self.memos.kinds.0 & Kinds::PART_OF != 0
    }

    /// The cards in exile that went there together from one merged or melded permanent,
    /// with the card in `slot` among them ([`Memo::ExiledFromPileOf`]): `slot` alone for any
    /// other card. One AND while no such card is in exile.
    #[inline]
    pub fn exiled_together(&self, slot: L::Slot) -> Bitboard<L::Slot> {
        if self.memos.kinds.0 & Kinds::EXILED_FROM_PILE == 0 {
            return Bitboard::single(slot);
        }
        self.exiled_together_now(slot)
    }

    /// The rare path of [`State::exiled_together`], out of line.
    #[cold]
    #[inline(never)]
    fn exiled_together_now(&self, slot: L::Slot) -> Bitboard<L::Slot> {
        let pile = self.memos.of(Kinds::EXILED_FROM_PILE);
        let leader = pile
            .iter()
            .find_map(|e| match e.memo {
                Memo::ExiledFromPileOf(leader) => (e.slot == slot).then_some(leader),
                Memo::Paid(_)
                | Memo::X(_)
                | Memo::ExiledWith(_)
                | Memo::ExiledUntil(_)
                | Memo::Chosen(_)
                | Memo::Really(_)
                | Memo::PutOntoBattlefieldWith(_)
                | Memo::PhasedOutUnder(_)
                | Memo::PartOf(_)
                | Memo::FaceDownUnder(_)
                | Memo::Morphed
                | Memo::ClassLevel(_)
                | Memo::Solved
                | Memo::CombatBand(_)
                | Memo::Prepared(_)
                | Memo::Cast => None,
            })
            .unwrap_or(slot);
        let mut together = Bitboard::single(slot);
        if self.object(leader).zone == Zone::Exile {
            together.insert(leader);
        }
        for e in pile {
            if e.memo == Memo::ExiledFromPileOf(leader) {
                together.insert(e.slot);
            }
        }
        together
    }

    /// The cards of the merged permanent in `slot` other than the one in its own slot, in the
    /// order they merged ([`Memo::PartOf`]): none for any other permanent. With no merged
    /// permanent in the game, one AND.
    #[inline]
    pub fn pile(&self, slot: L::Slot) -> impl Iterator<Item = L::Slot> + '_ {
        self.memos
            .of(Kinds::PART_OF)
            .iter()
            .filter(move |e| e.memo == Memo::PartOf(slot))
            .map(|e| e.slot)
    }
}
