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
//! A second, smaller table keeps what a spell was as it left the stack without resolving,
//! for an ability of its own that copies it ([`DepartedSpell`]).
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
use crate::state::{PendingTrigger, StackEntry, State};
use crate::table::{Capability, GameTable};

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

/// The rows of [`DepartedSpell`] a [`Small`] game holds: more spells than this leaving the
/// stack at once while abilities that copy them wait stalls the game into [`crate::Large`].
pub const DEPARTED_SPELLS: usize = 2;

/// What a spell was as it last existed on the stack, kept as it left without resolving
/// (countered, bounced, exiled) while an ability of its own that copies it waits or is on the
/// stack, as a storm trigger is. That ability copies it from this
/// ([CR 113.7a]): "Once activated or triggered, an ability exists on the stack
/// independently of its source." [CR 608.2h]: "If an effect requires information from the
/// game (such as the number of creatures on the battlefield), the answer is determined only
/// once, when the effect is applied. If the effect requires information from a specific
/// object, including the source of the ability itself, the effect uses the current
/// information of that object if it's in the public zone it was expected to be in; if it's
/// no longer in that zone, or if the effect has moved it from a public zone to a hidden zone,
/// the effect uses the object's last known information." The entry holds what a copy takes
/// besides the copiable values ([CR 707.10]): its card, face, targets, X and casting options
/// paid. Its object is no longer current.
///
/// [CR 113.7a]: https://yawgatog.com/resources/magic-rules/#R1137a
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
/// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DepartedSpell<L: Layout = Small> {
    pub entry: StackEntry<L>,
    pub copiable: Copiable<L>,
}

impl Copy for DepartedSpell<Small> {}

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
            self.object(slot).on_battlefield() && !self.flags.derived_dirty,
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
            copiable: self.copiable(slot),
            attached_to: o.attached_to.map(|to| self.object_ref(to)),
        };
        self.last_known.push(row)
    }

    /// What `spell`, a spell that left the stack without resolving, was there, if an ability
    /// of its own that copies it was waiting or on the stack as it left ([`DepartedSpell`]).
    #[inline]
    pub fn departed_spell(&self, spell: ObjectRef<L>) -> Option<&DepartedSpell<L>> {
        (self.departed_spells.iter()).find(|row| row.entry.spell() == Some(spell))
    }

    /// The rows of [`DepartedSpell`], in the order they were written.
    #[inline]
    pub fn departed_spells(&self) -> &[DepartedSpell<L>] {
        self.departed_spells.as_slice()
    }

    /// Keeps `entry`, a spell's, just taken off the stack while its card is still in the stack
    /// zone, if a game whose cards may copy their own spells has a waiting trigger or a stack
    /// entry whose source is that spell. In any other game, one AND.
    pub(crate) fn keep_departed_spell(
        &mut self,
        t: &GameTable,
        entry: &StackEntry<L>,
    ) -> Result<(), Full> {
        if !t.can(Capability::CopiesItself) {
            return Ok(());
        }
        let Some(spell) = entry.spell() else {
            return Ok(());
        };
        let named = self.triggers.iter().any(|tr| tr.of.source == Some(spell))
            || self.stack.iter().any(|e| e.source() == Some(spell));
        if !named {
            return Ok(());
        }
        let row = DepartedSpell {
            entry: entry.clone(),
            copiable: self.copiable(spell.slot),
        };
        self.departed_spells.push(row)
    }

    /// Repairs the ability targets of the kept spells after the stack entry at `removed` left
    /// from below the top, as for the entries above it: each kept spell's targets named only
    /// places below where it was.
    pub(crate) fn repair_departed_targets(&mut self, removed: usize) {
        for row in self.departed_spells.as_mut_slice() {
            for target in row.entry.targets.as_mut_slice() {
                *target = target.after_removal(removed);
            }
        }
    }

    /// Forgets the rows that no waiting trigger, stack entry or delayed trigger names, and
    /// the kept spells no waiting trigger or stack entry has as its source: the rules call it
    /// as a player would receive priority, when no resolution is under way. With no row, two
    /// comparisons.
    #[inline]
    pub fn forget_last_known(&mut self) {
        if !self.last_known.is_empty() {
            self.forget_unnamed_rows();
        }
        if !self.departed_spells.is_empty() {
            self.forget_departed_spells();
        }
    }

    #[cold]
    #[inline(never)]
    fn forget_departed_spells(&mut self) {
        let mut kept: L::Queue<DepartedSpell<L>, DEPARTED_SPELLS> = Default::default();
        for row in self.departed_spells.iter() {
            let spell = row.entry.spell();
            if self.triggers.iter().any(|tr| tr.of.source == spell)
                || self.stack.iter().any(|e| e.source() == spell)
            {
                // A subset of a queue of the same capacity.
                kept.push_bounded(row.clone());
            }
        }
        self.departed_spells = kept;
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
        let spells = self.departed_spells.as_slice();
        for (i, row) in spells.iter().enumerate() {
            let spell = row.entry.spell();
            check(spell.is_some(), "a kept spell's entry is a spell's", None)?;
            check(
                spells[..i].iter().all(|other| other.entry.spell() != spell),
                "two kept entries of one spell",
                None,
            )?;
        }
        Ok(())
    }
}
