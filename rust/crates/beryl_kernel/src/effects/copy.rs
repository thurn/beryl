//! Copy effects: the entries of the Effects store that give an object the copiable values of
//! another, with the exceptions they were made with ([`Continuous::Copy`]).
//!
//! A copy takes what it copies ([CR 707.2]): "When copying an object, the copy acquires the
//! copiable values of the original object's characteristics". The object's definition and face hold the values it copied, so
//! [`State::copiable`] reads what it reads of any object, and [`Flag::Copy`] says an entry
//! holds the rest: the exceptions ([CR 707.9b]): "The final set of values for that
//! characteristic becomes part of the copiable values of the copy."
//!
//! Built so far: the copy effect a token is created with, which makes it a copy of what it
//! copied for as long as it exists ([CR 111.3]): "The characteristic values defined this way
//! are functionally equivalent to the characteristic values that are printed on a card; for
//! example, they define the token's copiable values." A card under a copy effect, which
//! keeps the card it really is to return to, is built with the first card that becomes one.
//!
//! [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
//! [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
//! [CR 111.3]: https://yawgatog.com/resources/magic-rules/#R1113

use super::{Affects, Continuous, ContinuousEffect, Effects, Kinds, Payload};
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::Layout;
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
    /// exceptions. Its face is set as it enters (CR 707.8a), and turns as its own does.
    pub(super) fn take_copied_values(&mut self, object: ObjectRef<L>, values: Copiable<L>) {
        debug_assert!(
            self.object(object.slot).is_token(),
            "only a token's copy effect is built: a card would need the card it really is kept"
        );
        let card = values.card();
        let o = self.object_mut(object.slot);
        o.card = card;
        o.flags.set(Flag::Copy, true);
        self.flags.derived_dirty = true;
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
