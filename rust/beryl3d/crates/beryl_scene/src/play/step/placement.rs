//! The groups each script step places again.

use beryl_protocol::protocol::{CardType, ObjectId, ObjectView, PlayerView, RowName, ZoneName};
use beryl_protocol::script::{Act, Fact, Subject};

use super::Motion;
use crate::play::index;
use crate::table::{
    Group, HandFits, NameIds, PileRules, Placement, Rest, RowChange, Table, Zones, Zooms, blocked,
    blocking_rows, reposed,
};

impl<'a> Motion<'a> {
    /// Applies `act` to the table and places again every group whose resting places it may
    /// have changed: the groups that held or now hold an object the act named, and the stack
    /// where the act may change it without naming an object on it (`restacks`). Their resting
    /// places go to `fresh` and their slots to `next`; `rests` is left as it was. Returns which
    /// groups it placed, or none if the table could not take the act.
    pub(super) fn place(
        &mut self,
        table: &Table,
        act: &'a Act,
        timed: bool,
    ) -> Option<[bool; Group::ALL.len()]> {
        let (change, lands_may_change) = self.placement_change(act);
        let touched = &mut self.touched;
        touched.clear();
        if self
            .played
            .apply_noting(act, &mut |slot| touched.push(slot))
            .is_err()
        {
            self.whole = true;
            // A failed act may have changed the table before it stopped.
            self.index = None;
            self.unplaced += 1;
            return None;
        }
        let mut dirty = [self.whole; Group::ALL.len()];
        // The resting places a script starts from may not number an ability the table played
        // over does, so its first step places the stack again whatever its act.
        dirty[Group::Stack.index()] |= self.first || index::restacks(act);
        self.follow_look(act, timed, &mut dirty);
        if self.touched.is_empty() && !self.first && !dirty.iter().any(|&changed| changed) {
            return Some(dirty);
        }
        let zones = index::step_zones(self.index.as_mut(), &self.played, act, &self.touched)
            .handled(self.look.handled())
            .arrived(&self.arrival);
        // The first step places again each group kept in the order its objects arrived.
        if self.first {
            dirty_in_object_order(&mut dirty);
        }
        for object in self.touched.iter().filter_map(|&slot| zones.get(slot)) {
            self.names.note(object);
        }
        for &slot in &self.touched {
            let was = self.groups.get(usize::from(slot.0)).copied().flatten();
            let now = zones.get(slot).and_then(|object| zones.group_of(object));
            if !self.first
                && let Some(group @ Group::Row { .. }) = now
                && was == Some(group.index())
                && let Some((record, change)) = change
                && record.slot == slot
            {
                let keep = match change {
                    RowChange::Unchanged => true,
                    RowChange::Layout => false,
                    RowChange::Piling | RowChange::Pose => {
                        !self.members[group.index()].iter().any(|&other| {
                            other != slot
                                && zones
                                    .get(other)
                                    .is_some_and(|card| card.name == record.name)
                        })
                    }
                };
                if keep {
                    // Only the card's own pose changes, so it alone is given a new resting
                    // place, and its row is not laid out again.
                    if change == RowChange::Pose
                        && let Some(rest) = self.rests.get(slot)
                    {
                        let rest = Rest {
                            pose: reposed(record, rest.pose),
                            ..*rest
                        };
                        self.fresh.set(slot, rest);
                        self.reposed.push(slot);
                    }
                    continue;
                }
            }
            if let Some(group) = was {
                dirty[group] = true;
            }
            if let Some(group) = now {
                dirty[group.index()] = true;
            }
            // A permanent that enters or leaves a row takes the cards that lie under it along
            // or lets them go to their own groups, though the act names none of them
            // (`place::under`).
            let row = |index: usize| matches!(Group::ALL[index], Group::Row { .. });
            if was != now.map(Group::index)
                && (was.is_some_and(row) || now.is_some_and(|g| row(g.index())))
            {
                for object in zones.naming_as_host(slot) {
                    let index = usize::from(object.slot.0);
                    if let Some(group) = self.groups.get(index).copied().flatten() {
                        dirty[group] = true;
                    }
                    if let Some(group) = zones.group_of(object) {
                        dirty[group.index()] = true;
                    }
                }
            }
        }
        if !self.touched.is_empty() || dirty[Group::Stack.index()] {
            self.piles.update(&zones, &mut dirty, lands_may_change);
        }
        let fit = (table, &zones, &self.piles, &self.names);
        zoom(&mut self.fresh.zooms, fit, &mut dirty);
        let output = (&mut self.fresh, &mut self.hands, &mut self.next[..]);
        let input = (&zones, &self.names, &self.piles, &self.rests);
        place_dirty(table, input, output, &mut dirty);
        Some(dirty)
    }

    /// Follows the look open over the table played as `act` changes it (`Look::follow`), with
    /// the order the table's hands list their cards in, and marks in `dirty` the groups whose
    /// cards it lifts or puts back.
    fn follow_look(&mut self, act: &Act, timed: bool, dirty: &mut [bool]) {
        self.look
            .follow((act, timed), (&self.played, &self.arrival), dirty);
    }

    /// Every object's resting place in `view`, the view the script leads to. The table played
    /// over should end as that view stands, so each group that holds the same objects in both
    /// keeps the resting places the steps left, and only the others are placed again. After a
    /// step the table could not take, `rests` may not match the table, so every group is.
    /// `abilities` numbers the abilities on `view`'s stack.
    pub(crate) fn placement_of(
        &self,
        table: &Table,
        view: &PlayerView,
        abilities: &[u32],
    ) -> Placement {
        if self.whole {
            return table.placement(view, abilities);
        }
        let played = self
            .index
            .as_ref()
            .map_or_else(
                || Zones::of_table(&self.played),
                |index| index.zones(&self.played),
            )
            .handled(self.look.handled())
            .arrived(&self.arrival);
        let view = Zones::of_view(view, abilities);
        table.placement_reusing(&view, (&played, &self.rests))
    }

    /// Whether the act changes a row in place or can change the land count for either side.
    fn placement_change(&self, act: &'a Act) -> (Option<(&'a ObjectView, RowChange)>, bool) {
        // These acts replace a record where it stands. A flight can carry an equal record
        // and still reorder the row. A set can end a motion even where the rest stays put.
        let in_place = match act {
            Act::Flip { .. }
            | Act::Tap { .. }
            | Act::Showcase { .. }
            | Act::Count { .. }
            | Act::Strike { .. }
            | Act::Streak { .. } => act.object(),
            _ => None,
        };
        let carried = act.object();
        let slot = carried.map(|object| object.slot).or(match act {
            Act::Vanish {
                subject: Subject::Object { slot },
                ..
            } => Some(*slot),
            _ => None,
        });
        let was = slot.and_then(|slot| self.played.objects.get(&slot));
        let change = in_place
            .zip(was)
            .map(|(now, was)| (now, RowChange::between(was, now)));
        let lands_may_change = self.first
            || self.whole
            || matches!(
                act,
                Act::Set {
                    fact: Fact::Forget { .. } | Fact::ShuffledLibrary { .. }
                }
            )
            || was.and_then(land_side) != carried.and_then(land_side);
        (change, lands_may_change)
    }
}

/// A land counts for its controller while it is on the battlefield.
fn land_side(object: &ObjectView) -> Option<beryl_protocol::protocol::Side> {
    (object.zone == ZoneName::Battlefield && object.types.contains(&CardType::Land))
        .then_some(object.controller)
}

/// Finds again how far each battlefield row is zoomed out (`place::rezoom`) when a row is placed
/// again, measuring only the rows placed again, since a row that grew or shrank may zoom every
/// row, and then marks every row in `dirty` if any row's zoom moved. `zooms` holds them as the
/// steps before left the table.
fn zoom(
    zooms: &mut Zooms,
    (table, zones, piles, names): (&Table, &Zones, &PileRules, &NameIds),
    dirty: &mut [bool],
) {
    let row = |index: usize| matches!(Group::ALL[index], Group::Row { .. });
    if (0..dirty.len()).any(|index| dirty[index] && row(index)) {
        let changed = |side, row| dirty[Group::Row { side, row }.index()];
        let now = table.rezoom(zooms, (zones, piles, names), changed);
        let moved = now != *zooms;
        // The fits of the rows measured again are kept even when no zoom moved.
        *zooms = now;
        if moved {
            for (index, dirty) in dirty.iter_mut().enumerate() {
                *dirty |= row(index);
            }
        }
    }
}

/// Marks in `dirty` every group kept in the order its objects arrived (`Group::in_object_order`).
fn dirty_in_object_order(dirty: &mut [bool]) {
    for group in Group::ALL
        .into_iter()
        .filter(|group| group.in_object_order())
    {
        dirty[group.index()] = true;
    }
}

/// Places each group of `zones` that `dirty` marks, and then again each row that holds a
/// blocker, once any creatures' row has been laid out again, since a blocker faces where its
/// attacker rests (`place::blocking_rows`), marking those rows in `dirty` too. Their resting
/// places go to `fresh` and their slots to `next`. A blocker reads where its attacker rests
/// from `fresh`, which holds only the groups placed in this step, so an attacker whose row is
/// not placed again lends it the place it keeps in `rests` while the rows are laid out.
fn place_dirty(
    table: &Table,
    (zones, names, piles, rests): (&Zones<'_>, &NameIds<'_>, &PileRules, &Placement),
    (fresh, hands, next): (&mut Placement, &mut HandFits, &mut [Vec<ObjectId>]),
    dirty: &mut [bool],
) {
    let mut lent = Vec::new();
    for attacker in blocked(zones) {
        if fresh.get(attacker).is_none()
            && let Some(rest) = rests.get(attacker)
        {
            fresh.set(attacker, *rest);
            lent.push(attacker);
        }
    }
    // By index, which `dirty` and `next` share with `Group::ALL`: zipping the groups with their
    // lists cost about 300 instructions a step.
    for (index, next) in next.iter_mut().enumerate() {
        if dirty[index] {
            let output = (&mut *fresh, &mut *hands);
            table.place_group((zones, names, piles), Group::ALL[index], output, next);
        }
    }
    let creatures = |index: usize| {
        matches!(
            Group::ALL[index],
            Group::Row {
                row: RowName::Creatures,
                ..
            }
        )
    };
    let relay = (0..dirty.len()).any(|index| dirty[index] && creatures(index));
    for group in blocking_rows(zones).filter(|_| relay) {
        let index = group.index();
        dirty[index] = true;
        let output = (&mut *fresh, &mut *hands);
        table.place_group((zones, names, piles), group, output, &mut next[index]);
    }
    // A lent place is given back unless its row was placed after all, which replaced it.
    for attacker in lent {
        let placed = zones
            .get(attacker)
            .and_then(|object| zones.group_of(object))
            .is_some_and(|group| dirty[group.index()]);
        if !placed {
            fresh.clear(attacker);
        }
    }
}
