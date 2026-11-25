//! The permissions to cast from a graveyard once each turn that permanents used this turn.
//! Implements: none. Stores which permanents used such a permission for the rules to read.

use super::{Kinds, Layout, Memo, State};

impl<L: Layout> State<L> {
    /// Whether the permanent in `slot` has used its permission to cast a spell from a
    /// graveyard this turn ([`Memo::CastFromGraveyard`]). In a game where no permanent has,
    /// one AND.
    #[inline]
    pub fn cast_from_graveyard_this_turn(&self, slot: L::Slot) -> bool {
        (self.memos).has(Kinds::CAST_FROM_GRAVEYARD, slot, Memo::CastFromGraveyard)
    }

    /// Ends every [`Memo::CastFromGraveyard`] as a turn begins: each permission to cast from
    /// a graveyard once each turn is unused again.
    pub(crate) fn forget_graveyard_casts(&mut self) {
        if self.memos.kinds.0 & Kinds::CAST_FROM_GRAVEYARD != 0 {
            let done = self.memos.end(|e| e.memo != Memo::CastFromGraveyard);
            self.settle_ended(done);
        }
    }
}
