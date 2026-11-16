//! The groups each script step places again.

use beryl_protocol::protocol::{CardType, ObjectView, ZoneName};
use beryl_protocol::script::{Act, Fact, Subject};

use super::Motion;
use crate::play::index;
use crate::table::{Group, RowChange};

impl<'a> Motion<'a> {
    /// Applies `act` to the table and places again every group whose resting places it may
    /// have changed: the groups that held or now hold an object the act named, and the stack
    /// where the act may change it without naming an object on it (`restacks`). Their resting
    /// places go to `fresh` and their slots to `next`; `rests` is left as it was. Returns which
    /// groups it placed, or none if the table could not take the act.
    pub(super) fn place(&mut self, act: &'a Act, timed: bool) -> Option<[bool; Group::ALL.len()]> {
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
        self.look.follow((act, timed), &self.played, &mut dirty);
        if self.touched.is_empty() && !self.first && !dirty.iter().any(|&changed| changed) {
            return Some(dirty);
        }
        let zones = index::step_zones(self.index.as_mut(), &self.played, act, &self.touched)
            .handled(self.look.handled())
            .arrived(&self.arrival);
        if self.first {
            for group in Group::ALL
                .into_iter()
                .filter(|group| group.in_object_order())
            {
                dirty[group.index()] = true;
            }
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
                    RowChange::Piling => !self.members[group.index()].iter().any(|&other| {
                        other != slot
                            && zones
                                .get(other)
                                .is_some_and(|card| card.name == record.name)
                    }),
                };
                if keep {
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
        // By index, which `dirty` and `next` share with `Group::ALL`: zipping the groups with
        // their lists cost about 300 instructions a step.
        for (index, next) in self.next.iter_mut().enumerate() {
            if dirty[index] {
                let zones = (&zones, &self.names, &self.piles);
                let output = (&mut self.fresh, &mut self.hands);
                (self.table).place_group(zones, Group::ALL[index], output, next);
            }
        }
        Some(dirty)
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
