//! Last-known information: what a permanent that left the battlefield was as it last existed
//! there, kept for the abilities that look back in time at it ([CR 603.10a]): "Some
//! zone-change triggers look back in time. These are leaves-the-battlefield abilities". For
//! those, [CR 603.10] says, "the game “looks back in time” to determine if those abilities
//! trigger, using the existence of those abilities and the appearance of objects immediately
//! prior to the event."
//!
//! The rows are a table of the state, keyed by the [`ObjectRef`] of the object that left,
//! which is no longer current: the object's new incarnation is a new object ([CR 400.7]).
//! A row is written only when an ability that looks back matched the object
//! ([`State::record_last_known`]), so a game whose permanents never die with such an ability
//! never writes one. A waiting trigger, a stack entry or a delayed trigger reads its row by
//! the object it names ([`State::last_known`]), and the rows no record names are forgotten
//! between priorities ([`State::forget_last_known`]).
//!
//! [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
//! [CR 603.10]: https://yawgatog.com/resources/magic-rules/#R60310
//! [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007

use crate::counters::Counters;
use crate::fixed::Full;
use crate::invariants::{Violation, check};
use crate::layout::{Layout, Queue, Small};
use crate::object::{Derived, ObjectRef};
use crate::query::Copiable;
use crate::state::{PendingTrigger, State, Zone};

/// The rows a [`Small`] game holds: a sweeper that kills more creatures with abilities that
/// look back than this stalls the game into [`crate::Large`], whose table grows.
pub const LAST_KNOWN: usize = 16;

/// What a permanent was as it last existed on the battlefield: its layer results, the three
/// counters it kept in fields, whether it was a token, its copiable values and what it was
/// attached to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LastKnown<L: Layout = Small> {
    /// The object that left, as it was on the battlefield: no longer current.
    pub object: ObjectRef<L>,
    pub derived: Derived<L>,
    pub counters: Counters,
    pub token: bool,
    pub copiable: Copiable<L>,
    pub attached_to: Option<ObjectRef<L>>,
}

impl<L: Layout> LastKnown<L> {
    /// Whether two rows say the same of the objects they are about, whichever those are:
    /// two triggers of one ability on two such objects are alike in what they read.
    #[inline]
    pub fn alike(&self, other: &LastKnown<L>) -> bool {
        LastKnown {
            object: other.object,
            ..*self
        } == *other
    }
}

impl<L: Layout> State<L> {
    /// The last-known information of `object`, a permanent that left the battlefield, if an
    /// ability that looks back at it kept it.
    #[inline]
    pub fn last_known(&self, object: ObjectRef<L>) -> Option<&LastKnown<L>> {
        self.last_known.iter().find(|row| row.object == object)
    }

    /// The rows of last-known information, in the order they were written.
    #[inline]
    pub fn last_known_rows(&self) -> &[LastKnown<L>] {
        self.last_known.as_slice()
    }

    /// Keeps the last-known information of the permanent in `slot`, which is about to leave
    /// the battlefield, from its layer results as they are now: once for each object, however
    /// many abilities look back at it. The layer results must be current.
    pub fn record_last_known(&mut self, slot: L::Slot) -> Result<(), Full> {
        debug_assert!(
            self.object(slot).zone == Zone::Battlefield && !self.flags.derived_dirty,
            "last-known information of a permanent with current layer results"
        );
        let object = self.object_ref(slot);
        if self.last_known(object).is_some() {
            return Ok(());
        }
        let o = self.object(slot);
        let row = LastKnown {
            object,
            derived: *self.derived(slot),
            counters: o.counters,
            token: o.is_token(),
            copiable: Copiable::of_object(o),
            attached_to: o.attached_to.map(|to| self.object_ref(to)),
        };
        self.last_known.push(row)
    }

    /// Forgets the rows that no waiting trigger, stack entry or delayed trigger names: the
    /// rules call it as a player would receive priority, when no resolution is under way.
    /// With no row, one comparison.
    #[inline]
    pub fn forget_last_known(&mut self) {
        if !self.last_known.is_empty() {
            self.forget_unnamed_rows();
        }
    }

    #[cold]
    #[inline(never)]
    fn forget_unnamed_rows(&mut self) {
        let named = |r: ObjectRef<L>| {
            let names =
                |tr: &PendingTrigger<L>| tr.of.source == Some(r) || tr.info.object == Some(r);
            self.triggers.iter().any(names)
                || self.delayed.iter().any(names)
                || self.stack.iter().any(|entry| {
                    entry.source() == Some(r) || entry.info().is_some_and(|i| i.object == Some(r))
                })
        };
        let mut kept: L::Queue<LastKnown<L>, LAST_KNOWN> = Default::default();
        for row in self.last_known.iter().filter(|row| named(row.object)) {
            // A subset of a queue of the same capacity.
            kept.push_bounded(*row);
        }
        self.last_known = kept;
    }

    /// The table's invariants: one row for each object, each of an object that is no
    /// longer current, as one that left the battlefield is not.
    pub(crate) fn check_last_known(&self) -> Result<(), Violation> {
        let rows = self.last_known.as_slice();
        for (i, row) in rows.iter().enumerate() {
            check(
                !self.is_current(row.object),
                "last-known information of an object still current",
                None,
            )?;
            check(
                rows[..i].iter().all(|other| other.object != row.object),
                "two rows of last-known information of one object",
                None,
            )?;
        }
        Ok(())
    }
}
