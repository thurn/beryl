//! The derived cache: the layers' results, which the rules compute and the kernel stores
//! (architecture §9; the crate's contract, "Mutators").
//!
//! Implements: none. Stores the results calculated by the layers.
//!
//! A mutator that changes a layer input marks the cache stale. [`State::rederive`] then
//! lends it to the one function that fills it, `beryl_engine`'s `layers::refresh`, and the
//! cache is current again once that function finishes. Nothing else writes the cache or
//! clears the mark; [`State::forget_derived`] sets it with no change, for a recompute to
//! compare against.

use std::ops::Deref;

use beryl_card::{AbilityIx, FaceIx};

use crate::event::{Listens, Replaces};
use crate::layout::{Layout, PerSeat, Queue, Table};
use crate::object::{AbilityMask, Derived, GrantRow};
use crate::rule_changes::Rules;
use crate::state::State;
use crate::static_ability::Inputs;

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
    /// The layer results so far, one for each permanent, in battlefield order: what the
    /// layers applied until now made of each, for an effect that finds what it applies to
    /// against them.
    #[inline]
    pub fn rows(&self) -> &[Derived<L>] {
        &self.0.derived.as_slice()[..self.0.battlefield.len()]
    }

    /// Whether the permanent at battlefield position `i` has the ability at `ability` of its
    /// face, as the layers applied so far left it: [`State::has_ability`] for the layers'
    /// own reads, before the results are stored.
    #[inline]
    pub fn has_ability_so_far(&self, i: usize, ability: AbilityIx) -> bool {
        !{ self.rows()[i].lost }.contains(ability)
    }

    /// Sets the effects that reach objects off the battlefield ([`crate::query::zone_characteristics`]) to
    /// `reaching`, in the order they apply. [`Full`] past [`crate::query::zone_characteristics::ELSEWHERE`] in
    /// a fixed layout, with the list holding those that fit.
    ///
    /// [`Full`]: crate::fixed::Full
    pub fn reach(
        &mut self,
        reaching: impl IntoIterator<Item = crate::query::zone_characteristics::Reach<L>>,
    ) -> Result<(), crate::fixed::Full> {
        self.0.elsewhere.clear();
        reaching
            .into_iter()
            .try_for_each(|reach| self.0.elsewhere.push(reach))
    }

    /// Empties the table of grants, for a run of the layers to write again
    /// ([`State::grant_rows`]).
    #[inline]
    pub fn clear_grants(&mut self) {
        self.0.grants.clear();
    }

    /// Adds `row` to the table of grants, after those so far, and returns its place: the bit
    /// of each permanent's [`Derived::granted`] that it grants the ability to. [`Full`] past
    /// [`crate::state::GRANTS`] in a fixed layout, or past the width of the layout's mask.
    ///
    /// [`Full`]: crate::fixed::Full
    #[inline]
    pub fn add_grant(&mut self, row: GrantRow<L>) -> Result<usize, crate::fixed::Full> {
        let k = self.0.grants.len();
        if k >= L::AbilityMask::WIDTH {
            return Err(crate::fixed::Full);
        }
        self.0.grants.push(row)?;
        Ok(k)
    }

    /// Removes the rows of the table of grants past the first `len`: for a trial of an
    /// effect, which puts the results back as they were ([`Rederive::results`]).
    #[inline]
    pub fn truncate_grants(&mut self, len: usize) {
        while self.0.grants.len() > len {
            self.0.grants.pop();
        }
    }

    /// Whether any row written to the table of grants so far grants a plain mana ability.
    #[inline]
    pub fn has_granted_mana(&self, t: &crate::table::GameTable) -> bool {
        self.0
            .grants
            .as_slice()
            .iter()
            .any(|row| row.ability(t).is_some_and(crate::table::is_simple))
    }

    /// The layer results to write, one for each permanent, in battlefield order.
    #[inline]
    pub fn results(&mut self) -> &mut [Derived<L>] {
        let len = self.0.battlefield.len();
        &mut self.0.derived.as_mut_slice()[..len]
    }

    /// Stores the results as current, with `listening`, the kinds of event the permanents have
    /// triggered abilities for, `replacing`, the kinds of event their static abilities replace,
    /// and `rules`, the rules their static abilities change for each
    /// player, by seat ([`State::rule_applies`]), `inputs`, what the permanents' static
    /// abilities read that no layer writes ([`Inputs`]), `removed`, whether an effect that
    /// removes abilities applied, for [`State::has_ability`], and `land_types`, whether an
    /// effect that changes a land's land types applied, for [`State::makes_card_mana`].
    /// It notes whether any permanent is face down, for [`State::any_face_down`], and that the
    /// state triggers are due to be matched ([`State::states_due`]).
    /// Control is settled now, so each face-down
    /// permanent's controller sees it from here on (see [CR 708.5]): "At any time, you may look at ...
    /// a face-down permanent you control".
    ///
    /// [CR 708.5]: https://yawgatog.com/resources/magic-rules/#R7085
    #[inline]
    pub fn finish(
        self,
        listening: Listens,
        replacing: Replaces,
        rules: PerSeat<Rules>,
        inputs: Inputs,
        removed: bool,
        land_types: bool,
    ) {
        let s = self.0;
        s.listening = listening;
        s.replacing = replacing;
        s.static_rules = Table::from_fn(|player| rules[player]);
        s.layer_inputs = inputs;
        let mut face_down = false;
        for i in 0..s.battlefield.len() {
            let slot = s.battlefield.as_slice()[i];
            if s.object(slot).face == FaceIx::DOWN {
                face_down = true;
                let controller = s.derived[i].controller;
                s.knowledge[controller.index()].insert(slot);
                if let Some(sharer) = s.sharer(controller) {
                    s.knowledge[sharer.index()].insert(slot);
                }
            }
        }
        s.flags.abilities_lost = removed;
        s.flags.mana_changed = removed || land_types || s.has_mana_changes();
        s.flags.face_down = face_down;
        s.flags.derived_dirty = false;
        // A stored result may change a state trigger's condition ([`State::states_due`]).
        s.states_due = true;
    }
}
