//! The counters on an object, each kind with its own count (plan K4; the crate's contract,
//! "Stores").
//!
//! [CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221): "Counters with the same
//! name or description are interchangeable." The kinds are the vocabulary's list
//! (`beryl_card::counter_kinds`), and an object has any number of them at once.
//!
//! The three kinds the rules themselves read are fields of the object ([`Counters`]): +1/+1
//! and -1/-1, which the layers apply to every creature (CR 613.4c), and loyalty, which every
//! planeswalker has (CR 306.5b). Every other kind is a row of a table keyed by object and
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

use beryl_card::CounterKind;

use crate::bitboard::Bitboard;
use crate::event::Event;
use crate::fit::{Misfit, room};
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::observation::Observation;
use crate::relayout::{Relayout, queue, slot};
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
pub struct Counters {
    /// [CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c): "Effects and
    /// counters that modify power and/or toughness (but don't set power and/or toughness to
    /// a specific number or value) are applied."
    pub plus_one: u8,
    pub minus_one: u8,
    /// [CR 306.5c](https://yawgatog.com/resources/magic-rules/#R3065c): "The loyalty of a
    /// planeswalker on the battlefield is equal to the number of loyalty counters on it."
    pub loyalty: u8,
}

impl Counters {
    pub(crate) const NONE: Counters = Counters {
        plus_one: 0,
        minus_one: 0,
        loyalty: 0,
    };

    /// The count of a kind kept in a field; `None` for a kind kept in the table.
    #[inline]
    fn field(self, kind: CounterKind) -> Option<u8> {
        match kind {
            CounterKind::PlusOne => Some(self.plus_one),
            CounterKind::MinusOne => Some(self.minus_one),
            CounterKind::Loyalty => Some(self.loyalty),
            _ => None,
        }
    }

    /// The same, to change.
    #[inline]
    fn field_mut(&mut self, kind: CounterKind) -> Option<&mut u8> {
        match kind {
            CounterKind::PlusOne => Some(&mut self.plus_one),
            CounterKind::MinusOne => Some(&mut self.minus_one),
            CounterKind::Loyalty => Some(&mut self.loyalty),
            _ => None,
        }
    }
}

/// A row: the counters of one kind on the object now in a slot. Never 0 of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Row<L: Layout = Small> {
    slot: L::Slot,
    kind: CounterKind,
    count: u8,
}

impl<L: Layout> Default for Row<L> {
    fn default() -> Self {
        Row {
            slot: L::Slot::default(),
            kind: CounterKind::ALL[0],
            count: 0,
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
        )
    }

    fn check(&self, s: &State<L>, _: &GameTable) -> Result<(), Violation> {
        for (i, row) in self.rows.iter().enumerate() {
            let at = Some(At::Slot(row.slot.index()));
            check(row.count > 0, "a row of no counters", at)?;
            check(
                Counters::NONE.field(row.kind).is_none(),
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
                count: row.count,
            }),
        }
    }
}

/// The counters on one object, of every kind: to read a count and to list them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountersOf<'a, L: Layout = Small> {
    fields: Counters,
    rows: CounterRows<'a, L>,
}

impl<L: Layout> CountersOf<'_, L> {
    /// How many counters of a kind are on the object
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)).
    #[inline]
    pub fn get(&self, kind: CounterKind) -> u8 {
        match self.fields.field(kind) {
            Some(n) => n,
            None => self.rows.get(kind),
        }
    }

    /// Each kind the object has a counter of, with its count, in the order of the
    /// vocabulary's list: the same for two objects with the same counters, however they came
    /// by them.
    pub fn iter(&self) -> impl Iterator<Item = (CounterKind, u8)> + '_ {
        CounterKind::ALL.iter().filter_map(|&kind| {
            let n = self.get(kind);
            (n > 0).then_some((kind, n))
        })
    }

    /// The counts as a value that outlives the object, for an effect that reads them after
    /// moving it.
    #[inline]
    pub fn held(&self) -> CountersHeld {
        let mut held = CountersHeld::default();
        held.0[CounterKind::PlusOne as usize] = self.fields.plus_one;
        held.0[CounterKind::MinusOne as usize] = self.fields.minus_one;
        held.0[CounterKind::Loyalty as usize] = self.fields.loyalty;
        for row in self.rows.iter() {
            held.0[row.kind as usize] = row.count;
        }
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
    fn get(&self, kind: CounterKind) -> u8 {
        self.iter()
            .find(|row| row.kind == kind)
            .map_or(0, |row| row.count)
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

/// How many counters of each kind an object had: its last known information
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h): "the effect uses the
/// object's last known information"), a count for each kind of the vocabulary's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountersHeld([u8; CounterKind::ALL.len()]);

impl CountersHeld {
    /// How many counters of a kind the object had.
    #[inline]
    pub fn get(&self, kind: CounterKind) -> u8 {
        self.0[kind as usize]
    }
}

impl Default for CountersHeld {
    fn default() -> Self {
        CountersHeld([0; CounterKind::ALL.len()])
    }
}

impl<L: Layout> State<L> {
    /// How many counters of a kind are on the object in `slot`
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)). A kind the object
    /// keeps in a field is one read; any other, with nothing stored about the object, one
    /// bit test more.
    #[inline]
    pub fn counters_of(&self, slot: L::Slot, kind: CounterKind) -> u8 {
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
    /// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226): "Some spells and
    /// abilities refer to counters being put on an object."). `layered` says whether the
    /// layers read them, which is the rules' to say, with the card in hand: the layer
    /// results are stale if so. [`Full`] if the kind is the first of its kind on the object
    /// and the table has no room for the row.
    #[inline]
    pub fn add_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Result<Event<L>, Full> {
        debug_assert!(self.is_current(object));
        let n = self.counters_of(object.slot, kind).saturating_add(amount);
        self.set_counters(object.slot, kind, n)?;
        if layered {
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
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(self.is_current(object));
        match self.object_mut(object.slot).counters.field_mut(kind) {
            Some(n) => *n -= amount,
            None => self.take_row(object.slot, kind, amount),
        }
        if layered {
            self.flags.derived_dirty = true;
        }
        Event::CountersRemoved {
            object,
            kind,
            amount,
        }
    }

    /// Has the object in `slot` hold exactly `n` counters of a kind, with no event: as a
    /// permanent that enters with counters does (CR 122.6), and as the board door arranges
    /// one.
    #[inline]
    pub(crate) fn set_counters(
        &mut self,
        slot: L::Slot,
        kind: CounterKind,
        n: u8,
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
    fn set_row(&mut self, slot: L::Slot, kind: CounterKind, n: u8) -> Result<(), Full> {
        let rows = &mut self.counters.rows;
        let found = rows
            .iter()
            .position(|row| row.slot == slot && row.kind == kind);
        match (found, n) {
            (None, 0) => {}
            (None, count) => {
                rows.push(Row { slot, kind, count })?;
                self.keyed.insert(slot);
            }
            (Some(i), 0) => self.end_row(i),
            (Some(i), count) => rows.as_mut_slice()[i].count = count,
        }
        Ok(())
    }

    /// Takes `amount` counters from the row of a kind the object in `slot` has that many
    /// of.
    #[inline(never)]
    fn take_row(&mut self, slot: L::Slot, kind: CounterKind, amount: u8) {
        let rows = &mut self.counters.rows;
        let i = rows
            .iter()
            .position(|row| row.slot == slot && row.kind == kind)
            .expect("counters to take");
        let row = &mut rows.as_mut_slice()[i];
        row.count -= amount;
        if row.count == 0 {
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
