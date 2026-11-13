//! The derived cache: the layers' results, which the rules compute and the kernel stores
//! (architecture §9; the crate's contract, "Mutators").
//!
//! A mutator that changes a layer input marks the cache stale. [`State::rederive`] then
//! lends it to the one function that fills it, `beryl_engine`'s `layers::refresh`, and the
//! cache is current again once that function finishes. Nothing else writes the cache or
//! clears the mark; [`State::forget_derived`] sets it with no change, for a recompute to
//! compare against.

use std::ops::Deref;

use beryl_card::FaceIx;

use crate::event::Listens;
use crate::layout::{Layout, PerSeat, Queue, Table};
use crate::object::Derived;
use crate::rule_changes::Rules;
use crate::state::State;

/// The stale cache, lent by [`State::rederive`] to be filled. It reads as the [`State`] it
/// borrows.
pub struct Rederive<'a, L: Layout>(&'a mut State<L>);

impl<L: Layout> State<L> {
    /// Lends the derived cache to be filled, if a change since it was last stored may have
    /// changed a layer result; otherwise nothing, and the cache is current.
    #[inline]
    pub fn rederive(&mut self) -> Option<Rederive<'_, L>> {
        if !self.flags.derived_dirty {
            return None;
        }
        Some(Rederive(self))
    }

    /// Marks the layer results stale, so the next refresh computes them again from base:
    /// for the reference check, which compares the two, and the tests of the invariants. It
    /// can cause a recompute and nothing else, so it needs no event.
    #[doc(hidden)]
    #[inline]
    pub fn forget_derived(&mut self) {
        self.flags.derived_dirty = true;
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

    /// Stores the results as current, with `listening`, the kinds of event the permanents have
    /// triggered abilities for, and `rules`, the rules their static abilities change for each
    /// player, by seat ([`State::rule_applies`]). Control is settled now, so each face-down
    /// permanent's controller sees it from here on ([CR 708.5]): "At any time, you may look at ...
    /// a face-down permanent you control".
    ///
    /// [CR 708.5]: https://yawgatog.com/resources/magic-rules/#R7085
    #[inline]
    pub fn finish(self, listening: Listens, rules: PerSeat<Rules>) {
        let s = self.0;
        s.listening = listening;
        s.static_rules = Table::from_fn(|player| rules[player]);
        for i in 0..s.battlefield.len() {
            let slot = s.battlefield.as_slice()[i];
            if s.object(slot).face == FaceIx::DOWN {
                s.knowledge[s.derived[i].controller.index()].insert(slot);
            }
        }
        s.flags.derived_dirty = false;
    }
}
