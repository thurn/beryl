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
use beryl_protocol::script::{Act, Fact, Subject, Table as Played};

use game_table::table::{Group, NO_GROUP, OnStack, Zones, names_host, on_stack, stack_slots};

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
    /// Slots of cards in the stack zone, sorted as the table lists its objects. A resolving
    /// spell can still be in this zone after its entry has left the stack.
    stack_slots: Vec<ObjectId>,
    /// Hands and exile piles, in the order the table lists their objects.
    object_order: [Vec<ObjectId>; Group::ALL.len()],
    /// Slots of cards that name a possible host, in the table's order by slot.
    tucked: Vec<ObjectId>,
}

/// Whether `act` changes stack entries apart from moving a spell's object. The caller
/// also checks whether any touched object joins, leaves or changes on the stack.
pub(super) fn restacks(act: &Act) -> bool {
    matches!(
        act,
        Act::Emerge { .. }
            | Act::Vanish {
                subject: Subject::Ability { .. },
                ..
            }
            | Act::Set {
                fact: Fact::Item { .. }
            }
    )
}

/// The items of `played`'s stack, as placement reads them.
fn stack_of(played: &Played, slots: &[ObjectId]) -> Vec<OnStack> {
    on_stack(
        played.stack.iter().map(|entry| (&entry.item, entry.id)),
        &[],
        slots.iter().copied(),
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
        let mut tucked = Vec::new();
        let mut object_order = [const { Vec::new() }; Group::ALL.len()];
        for object in played.objects.values() {
            if let Some(group) = Group::of(object) {
                groups[usize::from(object.slot.0)] = group.number();
                if group.in_object_order() {
                    object_order[group.index()].push(object.slot);
                }
            }
            if names_host(object) {
                tucked.push(object.slot);
            }
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
        let stack_slots: Vec<_> = stack_slots(played.objects.values()).collect();
        Self {
            objects: same.then_some(objects),
            groups,
            stack: stack_of(played, &stack_slots),
            stack_slots,
            object_order,
            tucked,
        }
    }

    /// Brings the index up to date after `act` changed `played`, given the slots it named.
    pub(super) fn update(&mut self, played: &Played, act: &'a Act, touched: &[ObjectId]) {
        let replaced = act.object();
        let mut lost = false;
        let stacked = Group::Stack.number();
        let mut restacked = restacks(act);
        for &slot in touched {
            let index = usize::from(slot.0);
            // A successful act carrying one object puts that exact record in its slot.
            // An order change or a multi-object act still reads the resulting table.
            let replacement = replaced.filter(|object| object.slot == slot);
            let now = replacement.or_else(|| played.objects.get(&slot));
            if self.groups.len() <= index {
                self.groups.resize(index + 1, NO_GROUP);
            }
            let group = group_number(now);
            let previous = self.groups[index];
            if previous != group {
                if let Some(group) = Group::ALL
                    .get(usize::from(previous))
                    .filter(|group| group.in_object_order())
                {
                    update_slots(&mut self.object_order[group.index()], slot, false);
                }
                if let Some(group) = Group::ALL
                    .get(usize::from(group))
                    .filter(|group| group.in_object_order())
                {
                    update_slots(&mut self.object_order[group.index()], slot, true);
                }
            }
            let was_on_stack = previous == stacked;
            let on_stack = group == stacked;
            if on_stack || was_on_stack {
                update_slots(&mut self.stack_slots, slot, on_stack);
                restacked = true;
            }
            let tucked = now.is_some_and(names_host);
            if !self.tucked.is_empty() || tucked {
                update_slots(&mut self.tucked, slot, tucked);
            }
            self.groups[index] = group;
            let Some(objects) = &mut self.objects else {
                continue;
            };
            // The object the slot held, if the act left it as it was, or the act's own.
            let found = match (replacement, now) {
                (None, None) => Some(None),
                (Some(object), _) => Some(Some(object)),
                (None, Some(now)) => objects
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|&kept| kept == now)
                    .or_else(|| shuffled_object(act, slot).filter(|&object| object == now))
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
            self.stack = stack_of(played, &self.stack_slots);
        }
    }

    /// The zones of `played`, the table this indexes.
    pub(super) fn zones<'z>(&'z self, played: &'z Played) -> Zones<'z> {
        let groups = (&self.groups[..], &self.tucked[..], &self.object_order);
        match &self.objects {
            Some(objects) => Zones::of_table_indexed(played, (objects, &self.stack), groups),
            None => Zones::of_table_grouped(played, groups),
        }
    }
}

/// Keeps a slot in a sorted list exactly while it belongs there.
fn update_slots(slots: &mut Vec<ObjectId>, slot: ObjectId, belongs: bool) {
    match slots.binary_search(&slot) {
        Ok(at) if !belongs => {
            slots.remove(at);
        }
        Err(at) if belongs => slots.insert(at, slot),
        _ => {}
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

/// The replacement record for a card in a shuffled library.
fn shuffled_object(act: &Act, slot: ObjectId) -> Option<&ObjectView> {
    let Act::Set {
        fact: Fact::ShuffledLibrary { cards, .. },
    } = act
    else {
        return None;
    };
    cards.iter().rev().find(|object| object.slot == slot)
}
