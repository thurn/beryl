//! Copy effects: the entries of the Effects store that give an object the copiable values of
//! another, with the exceptions they were made with ([`Continuous::Copy`]).
//!
//! A copy takes what it copies ([CR 707.2]): "When copying an object, the copy acquires the
//! copiable values of the original object's characteristics". The object's definition and face hold the values it copied, so
//! [`State::copiable`] reads what it reads of any object, and [`Flag::Copy`] says an entry
//! holds the rest: the exceptions ([CR 707.9b]): "The final set of values for that
//! characteristic becomes part of the copiable values of the copy."
//!
//! A token's copy effect makes it a copy of what it copied for as long as it exists
//! ([CR 111.3]): "The characteristic values defined this way are functionally equivalent to
//! the characteristic values that are printed on a card; for example, they define the token's
//! copiable values." The zone move carries it to the token's next incarnation. A card under a
//! copy effect, such as one that entered as a copy, remembers the card it really is
//! ([`Memo::Really`]), and is that card again as the zone move ends the effect with the rest
//! of what the stores hold about it.
//!
//! [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
//! [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
//! [CR 111.3]: https://yawgatog.com/resources/magic-rules/#R1113

use super::{Affects, Continuous, ContinuousEffect, Effects, Kinds, Payload};
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::Layout;
use crate::memos::Memo;
use crate::object::{Flag, ObjectRef};
use crate::query::Copiable;
use crate::state::State;
use crate::store::Duration;

/// A copy effect as the zone move carries it to an object's next incarnation: all of its
/// entry but the object it affects.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Carried<L: Layout> {
    source: L::Card,
    timestamp: u16,
    duration: Duration,
    values: Copiable<L>,
}

impl<L: Layout> Effects<L> {
    /// The latest copy effect on the object in `slot`, of those still on it: the one layer 1
    /// applies last, and so the one whose values it has ([CR 613.7]): "An effect with an
    /// earlier timestamp is applied before an effect with a later timestamp."
    ///
    /// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
    fn latest_copy(&self, slot: L::Slot) -> Option<&ContinuousEffect<L>> {
        self.of(Kinds::COPY)
            .iter()
            .filter(|e| {
                matches!(e.payload, Payload::Copy(_))
                    && matches!(e.affected, Affects::Object { slot: s, .. } if s == slot)
            })
            .max_by_key(|e| e.timestamp)
    }
}

impl<L: Layout> State<L> {
    /// The copiable values the latest copy effect on the object in `slot` gives it. Asked
    /// only of an object with [`Flag::Copy`], which has one.
    #[cold]
    #[inline(never)]
    pub(crate) fn copy_effect(&self, slot: L::Slot) -> Copiable<L> {
        if let Some(Payload::Copy(values)) = self.effects.latest_copy(slot).map(|e| e.payload) {
            values
        } else {
            debug_assert!(false, "a copy with no copy effect");
            Copiable::default()
        }
    }

    /// The copy effect a token keeps as it changes zones, read before the move ends what the
    /// stores hold about it: a token copy is a copy wherever it is until it ceases to exist
    /// ([CR 111.3]), which is what its values were defined as.
    ///
    /// [CR 111.3]: https://yawgatog.com/resources/magic-rules/#R1113
    #[cold]
    #[inline(never)]
    pub(crate) fn carried_copy(&self, slot: L::Slot) -> Option<Carried<L>> {
        debug_assert!(self.object(slot).is_token());
        let e = self.effects.latest_copy(slot)?;
        let Payload::Copy(values) = e.payload else {
            return None;
        };
        Some(Carried {
            source: e.source,
            timestamp: e.timestamp,
            duration: e.duration,
            values,
        })
    }

    /// What the zone move keeps of the copy effect on the object in `slot` as it leaves: a
    /// token's effect to carry to its next incarnation ([`State::carried_copy`]), or the card a
    /// card under the effect really is, which it becomes again ([`Memo::Really`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn leaving_copy(&self, slot: L::Slot) -> (Option<Carried<L>>, Option<L::Card>) {
        if self.object(slot).is_token() {
            (self.carried_copy(slot), None)
        } else {
            (None, Some(self.really_is(slot)))
        }
    }

    /// Puts a carried copy effect back on the object now in `slot`, with the timestamp it
    /// began with.
    #[cold]
    #[inline(never)]
    pub(crate) fn carry_copy(&mut self, slot: L::Slot, carried: Carried<L>) -> Result<(), Full> {
        let object = self.object_ref(slot);
        let effect = Continuous::Copy(object, carried.values);
        self.push_effect(carried.source, effect, carried.duration, carried.timestamp)
    }

    /// Makes the object `object` take the values a copy effect starting on it gives it: its
    /// definition becomes the one it copied, and [`Flag::Copy`] says the entry holds its
    /// exceptions. A card first remembers the card it really is ([`Memo::Really`]). Its face
    /// is set as it enters, and turns as its own does. [`Full`] if the memo had no room.
    pub(super) fn take_copied_values(
        &mut self,
        object: ObjectRef<L>,
        values: Copiable<L>,
    ) -> Result<(), Full> {
        let o = *self.object(object.slot);
        if !o.is_token() && !o.flags.has(Flag::Copy) {
            self.remember(object.slot, Memo::Really(o.card))?;
        }
        let o = self.object_mut(object.slot);
        o.card = values.card();
        o.flags.set(Flag::Copy, true);
        self.flags.derived_dirty = true;
        Ok(())
    }

    /// The card a card under a copy effect really is, which the zone move makes it again as
    /// the effect ends ([`Memo::Really`]): read before the move ends the memo.
    #[cold]
    #[inline(never)]
    pub(crate) fn really_is(&self, slot: L::Slot) -> L::Card {
        debug_assert!(!self.object(slot).is_token());
        self.really(slot).unwrap_or_else(|| {
            debug_assert!(
                false,
                "a card under a copy effect with no memo of what it is"
            );
            self.object(slot).card
        })
    }
}

/// The invariant of the copy effects: an object has [`Flag::Copy`] exactly when a copy effect
/// is on it, and its definition is the one its latest copy effect gives it.
pub(super) fn check_copies<L: Layout>(effects: &Effects<L>, s: &State<L>) -> Result<(), Violation> {
    for e in effects.of(Kinds::COPY) {
        if let (Affects::Object { slot, .. }, Payload::Copy(_)) = (e.affected, e.payload) {
            check(
                s.object(slot).flags().has(Flag::Copy),
                "a copy effect on an object not marked a copy",
                Some(At::Slot(slot.index())),
            )?;
        }
    }
    for slot in L::Slot::all() {
        let o = s.object(slot);
        if !o.flags().has(Flag::Copy) {
            continue;
        }
        let at = Some(At::Slot(slot.index()));
        let latest = effects.latest_copy(slot);
        check(latest.is_some(), "a copy with no copy effect", at)?;
        check(
            o.is_token() || s.really(slot).is_some(),
            "a card under a copy effect does not remember the card it is",
            at,
        )?;
        if let Some(Payload::Copy(values)) = latest.map(|e| e.payload) {
            check(
                values.card() == o.card,
                "a copy's definition is not the one its copy effect gives it",
                at,
            )?;
        }
    }
    Ok(())
}
