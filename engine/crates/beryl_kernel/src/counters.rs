//! The counters on an object, each kind with its own count.
//!
//! [CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221): "Counters with the same
//! name or description are interchangeable." The kinds are the vocabulary's list
//! (`beryl_card::counter_kinds`), and an object has any number of them at once.
//!
//! The three kinds the rules themselves read are fields of the object ([`Counters`]): +1/+1
//! and -1/-1, which the layers apply to every creature (see CR 613.4c), and loyalty, which every
//! planeswalker has (see CR 306.5b). Every other kind is a row of a table keyed by object and
//! kind ([`CounterTable`]), since 255 objects cannot each carry a field for every kind. The
//! rules never see the difference: [`State::add_counters`], [`State::remove_counters`] and
//! [`State::counters_of`] take any kind.
//!
//! A row ends as its count reaches 0, and as its object leaves its zone, with everything
//! else stored about it
//! ([CR 122.2](https://yawgatog.com/resources/magic-rules/#R1222): "Counters on an object
//! are not retained if that object moves from one zone to another. The counters are not
//! 'removed'; they simply cease to exist."). The zone move pays for the table the bit test
//! of the keyed slots it pays for every store.
//!
//! Implements: CR 122.1, CR 122.2

use beryl_card::{CounterKind, counter_kinds};

use crate::bitboard::Bitboard;
use crate::event::Event;
use crate::fit::{Misfit, fits_number, room};
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Number, Queue, Small};
use crate::object::ObjectRef;
use crate::observation::Observation;
use crate::reference::Current;
use crate::relayout::{Relayout, number, queue, slot};
use crate::state::{State, Zone};
use crate::store::{Duration, Ended, Store};
use crate::table::GameTable;

/// The rows a [`Small`] game holds at once. One past it stalls the game into
/// [`crate::Large`], whose table grows.
pub const COUNTERS: usize = 32;

/// The counters of the kinds the rules themselves read, which an object keeps in fields.
/// Every other kind is a row of the [`CounterTable`]; [`State::counters_of`] and
/// [`State::counters`] answer for any kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Counters<L: Layout = Small> {
    /// See [CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c): "Effects and
    /// counters that modify power and/or toughness (but don't set power and/or toughness to
    /// a specific number or value) are applied."
    pub plus_one: L::Count,
    pub minus_one: L::Count,
    /// See [CR 306.5c](https://yawgatog.com/resources/magic-rules/#R3065c): "The loyalty of a
    /// planeswalker on the battlefield is equal to the number of loyalty counters on it."
    pub loyalty: L::Count,
}

impl<L: Layout> Counters<L> {
    pub(crate) const NONE: Counters<L> = Counters {
        plus_one: L::Count::ZERO,
        minus_one: L::Count::ZERO,
        loyalty: L::Count::ZERO,
    };

    /// The count of a kind kept in a field. Other kinds are kept in the table.
    #[inline]
    fn field(mut self, kind: CounterKind) -> Option<L::Count> {
        self.field_mut(kind).copied()
    }

    /// Whether the three counter fields fit layout `M`.
    pub(crate) fn fits<M: Layout>(self) -> Result<(), Misfit> {
        fits_number::<M::Count>("+1/+1 counters", self.plus_one.to_i64())?;
        fits_number::<M::Count>("-1/-1 counters", self.minus_one.to_i64())?;
        fits_number::<M::Count>("loyalty counters", self.loyalty.to_i64())
    }

    /// These counters in layout `M`.
    pub(crate) fn relayout<M: Layout>(self) -> Counters<M> {
        Counters {
            plus_one: number(self.plus_one),
            minus_one: number(self.minus_one),
            loyalty: number(self.loyalty),
        }
    }
}

// Only these three kinds have fields. New kinds from the catalog use the table.
macro_rules! counter_field {
    ($counters:ident, PlusOne) => {
        Some(&mut $counters.plus_one)
    };
    ($counters:ident, MinusOne) => {
        Some(&mut $counters.minus_one)
    };
    ($counters:ident, Loyalty) => {
        Some(&mut $counters.loyalty)
    };
    ($counters:ident, $other:ident) => {
        None
    };
}

macro_rules! counter_fields {
    ($($(#[$doc:meta])* $kind:ident = $name:literal),* $(,)?) => {
        impl<L: Layout> Counters<L> {
            #[inline]
            fn field_mut(&mut self, kind: CounterKind) -> Option<&mut L::Count> {
                match kind {
                    $(CounterKind::$kind => counter_field!(self, $kind)),*
                }
            }
        }
    };
}

counter_kinds!(counter_fields);

/// A row: the counters of one kind on the object now in a slot. Never 0 of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Row<L: Layout = Small> {
    slot: L::Slot,
    kind: CounterKind,
    count: L::Count,
}

impl<L: Layout> Default for Row<L> {
    fn default() -> Self {
        Row {
            slot: L::Slot::default(),
            kind: CounterKind::ALL[0],
            count: L::Count::ZERO,
        }
    }
}

/// The table of the counters no object keeps in a field: a row for each kind on each
/// object, in the order the first counter of the kind was put there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterTable<L: Layout = Small> {
    rows: L::Queue<Row<L>, COUNTERS>,
}

impl Copy for CounterTable<Small> {}

/// A row is 3 bytes in [`Small`] and the table at most 104: the state's room (the crate's
/// contract, "Stores") counts on it.
const _: () = assert!(size_of::<Row<Small>>() == 3 && size_of::<CounterTable<Small>>() <= 104);

impl<L: Layout> std::hash::Hash for CounterTable<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.rows.hash(h);
    }
}

impl<L: Layout> Store<L> for CounterTable<L> {
    fn empty() -> Self {
        CounterTable {
            rows: Default::default(),
        }
    }

    fn keys(&self) -> Bitboard<L::Slot> {
        self.rows.iter().map(|row| row.slot).collect()
    }

    /// None: a row is a count of one kind of counter on one object.
    #[inline]
    fn names(&self) -> Bitboard<L::Slot> {
        Bitboard::EMPTY
    }

    /// No layer input ends here: the layers read the counters of permanents alone, and a
    /// permanent that ends has left the battlefield, which marks their results stale.
    fn forget(&mut self, slot: L::Slot) -> Ended {
        let before = self.rows.len();
        self.rows.retain(|row| row.slot != slot);
        Ended {
            any: self.rows.len() != before,
            layer_input: false,
        }
    }

    /// Nothing: counters stay until they are removed or their object ends.
    #[inline]
    fn expire(&mut self, _: Duration) -> Ended {
        Ended::default()
    }

    fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        room(
            "the counters",
            self.rows.len(),
            <M::Queue<Row<M>, COUNTERS> as Queue<_>>::CAPACITY,
        )?;
        for row in self.rows.iter() {
            fits_number::<M::Count>("counters", row.count.to_i64())?;
        }
        Ok(())
    }

    fn check(&self, s: &State<L>, _: &GameTable) -> Result<(), Violation> {
        for (i, row) in self.rows.iter().enumerate() {
            let at = Some(At::Slot(row.slot.index()));
            check(row.count > L::Count::ZERO, "a row of no counters", at)?;
            check(
                Counters::<L>::NONE.field(row.kind).is_none(),
                "a row of a kind of counter the object keeps in a field",
                at,
            )?;
            check(
                s.object(row.slot).zone != Zone::Nowhere,
                "counters on an object in no zone",
                at,
            )?;
            let again = |earlier: &Row<L>| earlier.slot == row.slot && earlier.kind == row.kind;
            check(
                !self.rows.as_slice()[..i].iter().any(again),
                "two rows of one kind of counter on one object",
                at,
            )?;
        }
        Ok(())
    }

    /// The counters on anything but a permanent: a checkpoint of the board records a
    /// permanent's counters with the permanent, and nothing else's.
    fn lingering(&self, s: &State<L>, _: &GameTable) -> Option<String> {
        self.rows
            .iter()
            .find(|row| s.object(row.slot).zone != Zone::Battlefield)
            .map(|row| format!("{} counters on a card off the battlefield", row.kind.name()))
    }
}

impl<L: Layout> Relayout for CounterTable<L> {
    type In<M: Layout> = CounterTable<M>;

    fn relayout<M: Layout>(&self) -> CounterTable<M> {
        CounterTable {
            rows: queue(self.rows.as_slice(), |row| Row {
                slot: slot(row.slot),
                kind: row.kind,
                count: number(row.count),
            }),
        }
    }
}

/// The counters on one object, of every kind: to read a count and to list them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountersOf<'a, L: Layout = Small> {
    fields: Counters<L>,
    rows: CounterRows<'a, L>,
}

impl<L: Layout> CountersOf<'_, L> {
    /// How many counters of a kind are on the object
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)).
    #[inline]
    pub fn get(&self, kind: CounterKind) -> L::Count {
        match self.fields.field(kind) {
            Some(n) => n,
            None => self.rows.get(kind),
        }
    }

    /// Each kind the object has a counter of, with its count, in the order of the
    /// vocabulary's list: the same for two objects with the same counters, however they came
    /// by them.
    pub fn iter(&self) -> impl Iterator<Item = (CounterKind, L::Count)> + '_ {
        CounterKind::ALL.iter().filter_map(|&kind| {
            let n = self.get(kind);
            (n > L::Count::ZERO).then_some((kind, n))
        })
    }

    /// The counts as a value that outlives the object, for an effect that reads them after
    /// moving it.
    #[inline]
    pub fn held(&self) -> CountersHeld<L> {
        let mut held = CountersHeld {
            fields: self.fields,
            rows: [None; HELD],
        };
        for (to, row) in held.rows.iter_mut().zip(self.rows.iter()) {
            *to = Some((row.kind, row.count));
        }
        debug_assert!(
            self.rows.iter().count() <= HELD,
            "an object has more kinds of counter than its last known information holds"
        );
        held
    }
}

/// The counters one object has that it keeps in no field: its rows of the table. What
/// [`State::signature`] compares beside the object, which says the rest.
#[derive(Clone, Copy, Debug)]
pub struct CounterRows<'a, L: Layout = Small> {
    /// The table's rows, or none where no store has an entry about the object.
    rows: &'a [Row<L>],
    slot: L::Slot,
}

impl<L: Layout> CounterRows<'_, L> {
    /// The object's rows.
    #[inline]
    fn iter(&self) -> impl Iterator<Item = &Row<L>> + '_ {
        let slot = self.slot;
        self.rows.iter().filter(move |row| row.slot == slot)
    }

    /// How many counters of a kind kept in the table the object has.
    #[inline]
    fn get(&self, kind: CounterKind) -> L::Count {
        self.iter()
            .find(|row| row.kind == kind)
            .map_or(L::Count::ZERO, |row| row.count)
    }
}

/// Equal when the two objects have the same number of counters of every kind of the table,
/// in whatever order the kinds were put on them.
impl<L: Layout> PartialEq for CounterRows<'_, L> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // Nearly always: nothing is stored about either object.
        if self.rows.is_empty() && other.rows.is_empty() {
            return true;
        }
        self.iter().count() == other.iter().count()
            && self.iter().all(|row| other.get(row.kind) == row.count)
    }
}

impl<L: Layout> Eq for CounterRows<'_, L> {}

/// The kinds of counter kept in the table that last-known information holds of one object:
/// far more than any object of the pool's cards has at once. A count per kind of the
/// vocabulary's list would be a quarter kilobyte in every resolution's registers.
const HELD: usize = 6;

/// How many counters of each kind an object had: its last known information
/// (see [CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h): "the effect uses the
/// object's last known information"), the kinds it keeps in fields and its rows of the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CountersHeld<L: Layout = Small> {
    fields: Counters<L>,
    rows: [Option<(CounterKind, L::Count)>; HELD],
}

impl<L: Layout> CountersHeld<L> {
    /// How many counters of a kind the object had.
    #[inline]
    pub fn get(&self, kind: CounterKind) -> L::Count {
        match self.fields.field(kind) {
            Some(n) => n,
            None => self
                .rows
                .iter()
                .flatten()
                .find(|&&(k, _)| k == kind)
                .map_or(L::Count::ZERO, |&(_, n)| n),
        }
    }
}

impl<L: Layout> State<L> {
    /// How many counters of a kind are on the object in `slot`
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)). A kind the object
    /// keeps in a field is one read; any other, with nothing stored about the object, one
    /// bit test more.
    #[inline]
    pub fn counters_of(&self, slot: L::Slot, kind: CounterKind) -> L::Count {
        match self.object(slot).counters.field(kind) {
            Some(n) => n,
            None => self.counter_rows(slot).get(kind),
        }
    }

    /// The counters on the object in `slot`, of every kind.
    #[inline]
    pub fn counters(&self, slot: L::Slot) -> CountersOf<'_, L> {
        CountersOf {
            fields: self.object(slot).counters,
            rows: self.counter_rows(slot),
        }
    }

    /// The counters the object in `slot` keeps in no field. With nothing stored about the
    /// object, one bit test.
    #[inline]
    pub(crate) fn counter_rows(&self, slot: L::Slot) -> CounterRows<'_, L> {
        CounterRows {
            rows: if self.keyed.contains(slot) {
                self.counters.rows.as_slice()
            } else {
                &[]
            },
            slot,
        }
    }

    /// Puts counters of a kind on an object
    /// (see [CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226): "Some spells and
    /// abilities refer to counters being put on an object."). Use a fresh
    /// [`Current`] from [`State::current`]. `layered` says whether the
    /// layers read them, which is the rules' to say, with the card in hand: the layer
    /// results are stale if so. [`Full`] if the kind is the first of its kind on the object
    /// and the table has no room for the row, or if the new count overflows [`Layout::Count`].
    #[inline]
    pub fn add_counters(
        &mut self,
        object: Current<ObjectRef<L>>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Result<Event<L>, Full> {
        let object = object.reference();
        let n = self
            .counters_of(object.slot, kind)
            .checked_add(L::Count::from(amount))
            .ok_or(Full)?;
        self.set_counters(object.slot, kind, n)?;
        if layered
            || self
                .layer_inputs
                .contains(crate::static_ability::Inputs::ANY_COUNTERS)
        {
            self.flags.derived_dirty = true;
        }
        Ok(Event::CountersAdded {
            object,
            kind,
            amount,
        })
    }

    /// Removes `amount` counters of a kind from an object that has that many. `layered` as
    /// for [`State::add_counters`].
    #[inline]
    #[must_use]
    pub fn remove_counters(
        &mut self,
        object: Current<ObjectRef<L>>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        let object = object.reference();
        let take = L::Count::from(amount);
        match self.object_mut(object.slot).counters.field_mut(kind) {
            Some(n) => *n -= take,
            None => self.take_row(object.slot, kind, take),
        }
        if layered
            || self
                .layer_inputs
                .contains(crate::static_ability::Inputs::ANY_COUNTERS)
        {
            self.flags.derived_dirty = true;
        }
        Event::CountersRemoved {
            object,
            kind,
            amount,
        }
    }

    /// Has the object in `slot` hold exactly `n` counters of a kind, with no event: as a
    /// permanent that enters with counters does (see CR 122.6), and as the board door arranges
    /// one.
    #[inline]
    pub(crate) fn set_counters(
        &mut self,
        slot: L::Slot,
        kind: CounterKind,
        n: L::Count,
    ) -> Result<(), Full> {
        match self.object_mut(slot).counters.field_mut(kind) {
            Some(field) => {
                *field = n;
                Ok(())
            }
            None => self.set_row(slot, kind, n),
        }
    }

    /// [`State::set_counters`] of a kind kept in the table: out of line, since the kinds
    /// every game puts on its permanents are fields.
    #[inline(never)]
    fn set_row(&mut self, slot: L::Slot, kind: CounterKind, n: L::Count) -> Result<(), Full> {
        let rows = &mut self.counters.rows;
        let found = rows
            .iter()
            .position(|row| row.slot == slot && row.kind == kind);
        match (found, n.is_zero()) {
            (None, true) => {}
            (None, false) => {
                rows.push(Row {
                    slot,
                    kind,
                    count: n,
                })?;
                self.keyed.insert(slot);
            }
            (Some(i), true) => self.end_row(i),
            (Some(i), false) => rows.as_mut_slice()[i].count = n,
        }
        Ok(())
    }

    /// Takes `amount` counters from the row of a kind the object in `slot` has that many
    /// of.
    #[inline(never)]
    fn take_row(&mut self, slot: L::Slot, kind: CounterKind, amount: L::Count) {
        let rows = &mut self.counters.rows;
        let i = rows
            .iter()
            .position(|row| row.slot == slot && row.kind == kind)
            .expect("counters to take");
        let row = &mut rows.as_mut_slice()[i];
        row.count -= amount;
        if row.count.is_zero() {
            self.end_row(i);
        }
    }

    /// Ends the row at `i`, whose last counter is gone.
    fn end_row(&mut self, i: usize) {
        self.counters.rows.remove(i);
        self.settle_ended(Ended {
            any: true,
            layer_input: false,
        });
    }
}

impl<'a, L: Layout> Observation<'a, L> {
    /// The counters on the object in `slot`. Public wherever the object is: a counter is a
    /// marker on it for every player to see (CR 122.1).
    pub fn counters(&self, slot: L::Slot) -> CountersOf<'a, L> {
        self.shown().counters(slot)
    }
}

#[cfg(test)]
mod tests;
