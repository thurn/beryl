//! What a face-down permanent remembers of how it was cast.
//!
//! Implements: none. Reads the memos of a face-down spell and the permanent it becomes.

use super::{Kinds, Layout, Memo, State};

impl<L: Layout> State<L> {
    /// Whether the face-down spell or permanent in `slot` is turned face up as it is used
    /// ([`Memo::TurnsUpWhenUsed`]). With nothing in the game that remembers it, one AND.
    #[inline]
    pub fn turns_up_when_used(&self, slot: L::Slot) -> bool {
        (self.memos).has(Kinds::TURNS_UP, slot, Memo::TurnsUpWhenUsed)
    }

    /// Whether the permanent in `slot` entered by resolving a face-down morph spell.
    pub fn morphed(&self, slot: L::Slot) -> bool {
        self.memos.has(Kinds::MORPHED, slot, Memo::Morphed)
    }
}
