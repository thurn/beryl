//! Combat bands in the memo store.
//!
//! Implements: CR 702.22e, CR 702.22f

use super::{Kinds, Memo, Memos};
use crate::fit::Misfit;
use crate::ids::SlotId as _;
use crate::layout::Layout;
use crate::state::State;

impl<L: Layout> State<L> {
    /// The declared bands, represented by one record per member in the fixed Memos queue.
    /// Each pair contains a member and the ID shared by all members of its band.
    pub fn band_members(&self) -> impl Iterator<Item = (L::Slot, L::Slot)> + '_ {
        self.memos.of(Kinds::COMBAT_BAND).iter().filter_map(|e| {
            if let Memo::CombatBand(band) = e.memo {
                Some((e.slot, band))
            } else {
                None
            }
        })
    }

    /// The declared band of an attacker, if any.
    pub fn combat_band(&self, member: L::Slot) -> Option<L::Slot> {
        self.band_members()
            .find_map(|(slot, band)| (slot == member).then_some(band))
    }

    /// Removes a member from its band, or ends all bands when combat ends.
    /// Under [CR 702.22e], "Once an attacking band has been announced, it lasts for the
    /// rest of combat". Under [CR 702.22f], "An attacking creature that’s removed from
    /// combat is also removed from the band it was in." A zone change also ends its memo.
    ///
    /// [CR 702.22e]: https://yawgatog.com/resources/magic-rules/#R70222e
    /// [CR 702.22f]: https://yawgatog.com/resources/magic-rules/#R70222f
    #[inline]
    pub(crate) fn forget_combat_bands(&mut self, member: Option<L::Slot>) {
        if self.memos.kinds.0 & Kinds::COMBAT_BAND != 0 {
            self.end_combat_bands(member);
        }
    }

    #[cold]
    #[inline(never)]
    fn end_combat_bands(&mut self, member: Option<L::Slot>) {
        let done = self.memos.end(|e| {
            !matches!(e.memo, Memo::CombatBand(_)) || member.is_some_and(|slot| slot != e.slot)
        });
        self.settle_ended(done);
    }
}

impl<L: Layout> Memos<L> {
    /// A band ID can still name a slot after its first member leaves combat.
    /// Check that it can be carried over even when the slot is now empty.
    pub(super) fn combat_fits<M: Layout>(&self) -> Result<(), Misfit> {
        for entry in self.of(Kinds::COMBAT_BAND) {
            if let Memo::CombatBand(band) = entry.memo
                && band.to::<M::Slot>().is_none()
            {
                return Err(Misfit::IndexTooWide {
                    index: band.index(),
                });
            }
        }
        Ok(())
    }
}
