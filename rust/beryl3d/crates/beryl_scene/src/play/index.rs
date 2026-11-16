//! What placement reads of the table a script plays over, kept up to date step by step, so that
//! a step indexes only the objects it names and not the whole table.
//!
//! The table played over keeps its objects in a map by slot, which a step's placement would
//! otherwise copy into a list by slot every step (`Zones::of_table`). The table a script starts
//! from shows the view before it, and each step leaves the objects it does not name as they were
//! and puts the ones it names as its act holds them, so every object of the table is equal to one
//! in that view or in one of the script's acts, both of which outlive the script's compiling: the
//! index borrows from those. Where that does not hold (a recording whose script did not replay to
//! its view), each step lists the table's objects again.

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView};
use beryl_protocol::script::{Act, Fact, Table as Played, Tally};

use crate::table::{Group, NO_GROUP, OnStack, Zones, on_stack};

/// The fewest steps a script needs for its index to be built: building it reads every object of
/// the table about four times as long as listing them for one step does, so a script of a step or
/// two lists them each step instead.
pub(super) const LEAST_STEPS: usize = 8;

/// The objects of the table a script plays over, and their groups, by slot.
pub(super) struct Index<'a> {
    /// Every object of the table, by slot, borrowed from an equal object in the view the script
    /// starts from or in one of its acts; none once a step left an object that neither holds,
    /// after which each step lists the table's objects again.
    objects: Option<Vec<Option<&'a ObjectView>>>,
    /// The `Group::number` of each object of the table (`Group::of`), or `NO_GROUP`, by slot,
    /// which a battlefield row finds its cards by.
    groups: Vec<u8>,
    /// The items of the table's stack (`on_stack`), listed again only after a step that may
    /// have changed them, since listing them reads every object of the table.
    stack: Vec<OnStack>,
}

/// Whether `act` may change the stack without naming an object on it: put an ability on it,
/// take one off it, or set a fact about it. Any other change to the stack moves a spell's card,
/// which the act names. An act that does neither, such as an arrow that opens over a full
/// stack, leaves every item of the stack where it was.
pub(super) fn restacks(act: &Act) -> bool {
    matches!(
        act,
        Act::Emerge { .. }
            | Act::Vanish { .. }
            | Act::Set { .. }
            | Act::Fly { .. }
            | Act::Appear { .. }
    )
}

/// The items of `played`'s stack, as placement reads them.
fn stack_of(played: &Played) -> Vec<OnStack> {
    on_stack(
        played.stack.iter().map(|entry| (&entry.item, entry.id)),
        &[],
        played.objects.values(),
    )
}

impl<'a> Index<'a> {
    /// The index of `played`, a table that shows `before`.
    pub(super) fn new(before: &'a PlayerView, played: &Played) -> Self {
        let end = played
            .objects
            .last_key_value()
            .map_or(0, |(slot, _)| usize::from(slot.0) + 1);
        let mut groups = vec![NO_GROUP; end];
        for object in played.objects.values() {
            groups[usize::from(object.slot.0)] = group_number(Some(object));
        }
        let end = before
            .objects
            .iter()
            .map(|object| usize::from(object.slot.0) + 1)
            .max()
            .unwrap_or(0);
        let mut objects = vec![None; end];
        for object in &before.objects {
            objects[usize::from(object.slot.0)] = Some(object);
        }
        // The table should hold the view's objects, but a table that a script before ended with
        // may not.
        let same = played.objects.len() == before.objects.len()
            && played.objects.values().all(|object| {
                objects.get(usize::from(object.slot.0)).copied().flatten() == Some(object)
            });
        Self {
            objects: same.then_some(objects),
            groups,
            stack: stack_of(played),
        }
    }

    /// Brings the index up to date after `act` changed `played`, given the slots it named.
    pub(super) fn update(&mut self, played: &Played, act: &'a Act, touched: &[ObjectId]) {
        let mut lost = false;
        let stacked = Group::Stack.number();
        let mut restacked = restacks(act);
        for &slot in touched {
            let index = usize::from(slot.0);
            let now = played.objects.get(&slot);
            if self.groups.len() <= index {
                self.groups.resize(index + 1, NO_GROUP);
            }
            let group = group_number(now);
            restacked |= group == stacked || self.groups[index] == stacked;
            self.groups[index] = group;
            let Some(objects) = &mut self.objects else {
                continue;
            };
            // The object the slot held, if the act left it as it was, or the act's own.
            let found = match now {
                None => Some(None),
                Some(now) => objects
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|&kept| kept == now)
                    .or_else(|| objects_of(act).iter().find(|&object| object == now))
                    .map(Some),
            };
            match found {
                Some(object) => {
                    if objects.len() <= index {
                        objects.resize(index + 1, None);
                    }
                    objects[index] = object;
                }
                None => lost = true,
            }
        }
        if lost {
            self.objects = None;
        }
        if restacked {
            self.stack = stack_of(played);
        }
    }

    /// The zones of `played`, the table this indexes.
    pub(super) fn zones<'z>(&'z self, played: &'z Played) -> Zones<'z> {
        match &self.objects {
            Some(objects) => Zones::of_table_indexed(played, (objects, &self.stack), &self.groups),
            None => Zones::of_table_grouped(played, &self.groups),
        }
    }
}

/// The zones of `played` after `act`, which named the slots `touched`, with `index` brought up to
/// date, or listed from `played` itself where the script keeps no index.
pub(super) fn step_zones<'z, 'a: 'z>(
    index: Option<&'z mut Index<'a>>,
    played: &'z Played,
    act: &'a Act,
    touched: &[ObjectId],
) -> Zones<'z> {
    match index {
        Some(index) => {
            index.update(played, act, touched);
            index.zones(played)
        }
        None => Zones::of_table(played),
    }
}

/// The `Group::number` of the group `object` belongs in, or `NO_GROUP`.
fn group_number(object: Option<&ObjectView>) -> u8 {
    object.and_then(Group::of).map_or(NO_GROUP, Group::number)
}

/// The objects `act` puts on the table as they are after it.
fn objects_of(act: &Act) -> &[ObjectView] {
    match act {
        Act::Fly { object, .. }
        | Act::Appear { object, .. }
        | Act::Flip { object }
        | Act::Tap { object }
        | Act::Showcase { object }
        | Act::Count {
            tally:
                Tally::Counters { object, .. }
                | Tally::NamedCounters { object, .. }
                | Tally::Stats { object }
                | Tally::Damage { object },
        }
        | Act::Set {
            fact: Fact::Object { object },
        } => std::slice::from_ref(object),
        Act::Vanish { object, .. } | Act::Strike { object, .. } | Act::Streak { object, .. } => {
            object.as_slice()
        }
        Act::Set {
            fact: Fact::ShuffledLibrary { cards, .. },
        } => cards,
        _ => &[],
    }
}
