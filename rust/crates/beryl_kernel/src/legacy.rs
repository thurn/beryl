//! The legacy accessor: raw writes of what milestone 4 (by-w8gv.7.1) deletes, which
//! therefore gets no typed mutator (the crate's contract, "The legacy accessor"). A search
//! for `legacy_mut` finds every caller milestone 4 has to move.

use beryl_card::CounterKind;

use crate::event::Event;
use crate::layout::Layout;
use crate::object::{Counters, ObjectFlags, ObjectRef};
use crate::state::State;

/// Lent by [`State::legacy_mut`]. Deleted by milestone 4 with the last of what it writes,
/// each of which a store replaces (by-w8gv.7.1):
///
/// | Behind the accessor | Written by | Replaced by |
/// | --- | --- | --- |
/// | the [`ObjectFlags::LEGACY`] flags: `EXILED_UNTIL`, `EXILED_WITH`, `LOYALTY_ACTIVATED` | [`Legacy::set_object_flags`] | memos of the exiled card in the Memos store; a tracker of the permanent |
/// | [`Counters`]' long-tail kind (`other_kind`, `other`) | [`Legacy::add_counters`], [`Legacy::remove_counters`], [`Legacy::set_counters`] | counter kinds generated from a list, each with its own count |
pub struct Legacy<'a, L: Layout>(&'a mut State<L>);

impl<L: Layout> State<L> {
    /// Lends the raw writes of what milestone 4 (by-w8gv.7.1) moves to memos and stores:
    /// [`Legacy`] lists them.
    #[inline]
    pub fn legacy_mut(&mut self) -> Legacy<'_, L> {
        Legacy(self)
    }
}

impl<L: Layout> Legacy<'_, L> {
    /// Sets or clears flags among [`ObjectFlags::LEGACY`] on an object. The layers read
    /// none of them.
    #[inline]
    pub fn set_object_flags(&mut self, slot: L::Slot, flags: u32, on: bool) {
        debug_assert_eq!(flags & !ObjectFlags::LEGACY, 0, "a flag with a mutator");
        self.0.object_mut(slot).flags.set(flags, on);
    }

    /// Puts counters of a long-tail kind on an object: [`State::add_counters`] for them.
    #[inline]
    #[must_use]
    pub fn add_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(Counters::is_long_tail(kind));
        self.0.put_counters(object, kind, amount, layered)
    }

    /// Removes `amount` counters of a long-tail kind from an object that has that many:
    /// [`State::remove_counters`] for them.
    #[inline]
    #[must_use]
    pub fn remove_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(Counters::is_long_tail(kind));
        self.0.take_counters(object, kind, amount, layered)
    }

    /// Sets how many counters of a long-tail kind an object has, as one that enters with
    /// them does ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)).
    #[inline]
    pub fn set_counters(&mut self, slot: L::Slot, kind: CounterKind, n: u8) {
        debug_assert!(Counters::is_long_tail(kind));
        *self.0.object_mut(slot).counters.get_mut(kind) = n;
    }
}
