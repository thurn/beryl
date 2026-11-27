//! What the Effects store holds about one object ([`EffectsOn`]), to compare two objects by.
//!
//! Implements: none. Reads stored effects to compare objects.

use super::{Affects, Effects, Payload};
use crate::layout::{Layout, Queue, Small};
use crate::state::State;
use crate::store::Duration;

/// What the Effects store holds about one object, to compare two objects by
/// ([`State::signature`]): each effect on it, alone or as a member of a locked-in set, as
/// what it does, for how long and of which card, in the order they began. Not when each
/// began: two objects that the same things were done to in the same order are affected
/// alike from here, whichever was first. The same things in another order are told apart,
/// as two layer changes may come to another result that way.
///
/// A permanent's layer results show what its effects make of it now and not what they
/// will: a creature that gets +1/+1 until end of turn and one that gets it for good are
/// the same size today and are not interchangeable.
///
/// Repeated `ModifyPt` effects from one source with one duration on one object merge into
/// one entry holding their sum ([`State::start_effect`]), so two creatures with the same
/// total bonus from the same source and duration compare equal here whether it came from
/// one effect or several.
#[derive(Clone, Copy, Debug)]
pub struct EffectsOn<'a, L: Layout = Small> {
    /// The store, or none where no store has an entry about the object.
    store: Option<&'a Effects<L>>,
    slot: L::Slot,
}

impl<L: Layout> EffectsOn<'_, L> {
    /// The effects on the object, in the order they began: the card each came from, what
    /// it does and how long it lasts.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (L::Card, Payload<L>, Duration)> + '_ {
        let slot = self.slot;
        self.store.into_iter().flat_map(move |store| {
            (store.entries.iter())
                .filter(move |e| match e.affected {
                    Affects::Object { slot: affected, .. } => affected == slot,
                    Affects::Set(set) => store.sets.members(set).contains(slot),
                    Affects::Player(_) | Affects::Game => false,
                })
                .map(|e| (e.source, e.payload, e.duration))
        })
    }
}

impl<L: Layout> PartialEq for EffectsOn<'_, L> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // Nearly always: nothing is stored about either object.
        if self.store.is_none() && other.store.is_none() {
            return true;
        }
        self.iter().eq(other.iter())
    }
}

impl<L: Layout> Eq for EffectsOn<'_, L> {}

impl<L: Layout> State<L> {
    /// The continuous effects on the object in `slot`: for [`State::signature`], tools,
    /// tests and a view. The rules ask by kind. With nothing stored about the object, one
    /// bit test.
    #[inline]
    pub fn effects_on(&self, slot: L::Slot) -> EffectsOn<'_, L> {
        EffectsOn {
            store: self.keyed.contains(slot).then_some(&self.effects),
            slot,
        }
    }
}
