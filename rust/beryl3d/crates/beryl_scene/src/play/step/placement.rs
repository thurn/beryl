//! The groups each script step places again.

use beryl_protocol::script::Act;

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
        let change = in_place.and_then(|now| {
            let was = self.played.objects.get(&now.slot)?;
            Some((now, RowChange::between(was, now)))
        });
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
        for (group, next) in Group::ALL.into_iter().zip(&mut self.next) {
            if dirty[group.index()] {
                let zones = (&zones, &self.names);
                self.table.place_group(zones, group, &mut self.fresh, next);
            }
        }
        Some(dirty)
    }
}
