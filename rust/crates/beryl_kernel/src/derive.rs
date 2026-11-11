//! The derived cache: the layers' results, which the rules compute and the kernel stores
//! (architecture §9; the crate's contract, "Mutators").
//!
//! A mutator that changes a layer input marks the cache stale. [`State::rederive`] then
//! lends it to the one function that fills it, `beryl_engine`'s `layers::refresh`, and the
//! cache is current again once that function finishes. Nothing else writes the cache or
//! clears the mark.

use std::ops::Deref;

use beryl_card::FaceIx;

use crate::layout::{Layout, Queue, Table};
use crate::object::Derived;
use crate::state::State;

/// The stale cache, lent by [`State::rederive`] to be filled. It reads as the [`State`] it
/// borrows.
pub struct Rederive<'a, L: Layout>(&'a mut State<L>);

impl<L: Layout> State<L> {
    /// Lends the derived cache to be filled, if a change since it was last stored may have
    /// changed a layer result; otherwise nothing, and the cache is current. The floating
    /// effects on objects that have left their zone end first: such an object no longer
    /// exists ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
    #[inline]
    pub fn rederive(&mut self) -> Option<Rederive<'_, L>> {
        if !self.flags.derived_dirty {
            return None;
        }
        self.drop_stale_effects();
        Some(Rederive(self))
    }
}

impl<L: Layout> Deref for Rederive<'_, L> {
    type Target = State<L>;

    #[inline]
    fn deref(&self) -> &State<L> {
        self.0
    }
}

impl<L: Layout> Rederive<'_, L> {
    /// The layer results to write, one for each permanent, in battlefield order.
    #[inline]
    pub fn results(&mut self) -> &mut [Derived<L>] {
        let len = self.0.battlefield.len();
        &mut self.0.derived.as_mut_slice()[..len]
    }

    /// Stores the results as current, with `listening`, the kinds of event the permanents
    /// have triggered abilities for. Control is settled now, so each face-down permanent's
    /// controller sees it from here on
    /// ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "At any time, you may
    /// look at ... a face-down permanent you control".
    #[inline]
    pub fn finish(self, listening: u16) {
        let s = self.0;
        s.listening = listening;
        for i in 0..s.battlefield.len() {
            let slot = s.battlefield.as_slice()[i];
            if s.object(slot).face == FaceIx::DOWN {
                s.knowledge[s.derived[i].controller.index()].insert(slot);
            }
        }
        s.flags.derived_dirty = false;
    }
}
