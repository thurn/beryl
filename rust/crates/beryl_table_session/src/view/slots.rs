use std::collections::{BTreeMap, BTreeSet, btree_map};

use beryl_session::protocol::ObjectId as Slot;
use beryl_table::state::ObjectId;
use beryl_table::{Change, Entry, Seat, Table};

/// A single viewer's stage slots. Use a new value when opening another table
/// or changing seats.
#[derive(Clone, Debug, Default)]
pub struct Slots {
    by_id: BTreeMap<ObjectId, Slot>,
    revision: Option<u32>,
    next: u16,
}

impl Slots {
    /// The slot of an object in the last published view.
    pub fn get(&self, id: ObjectId) -> Option<Slot> {
        self.by_id.get(&id).copied()
    }

    pub(super) fn update(&mut self, table: &Table, seat: Seat, entries: &[Entry]) -> Option<()> {
        let first = self.revision.map_or(entries.len(), |revision| {
            entries.partition_point(|entry| entry.n <= revision)
        });
        // Keep previous slots reserved until the new view is built. A card
        // that loses its identity cannot immediately receive its old slot.
        let mut used: BTreeSet<_> = self.by_id.values().copied().collect();
        let mut slots = self.by_id.clone();
        for entry in &entries[first..] {
            Self::follow(&mut slots, &entry.changes, seat);
        }
        slots.retain(|id, _| table.objects.contains_key(id));
        let mut next = self.next;
        for &id in table.objects.keys() {
            if let btree_map::Entry::Vacant(entry) = slots.entry(id) {
                let slot = (0..=u16::MAX).find_map(|_| {
                    let candidate = Slot(next);
                    next = next.wrapping_add(1);
                    used.insert(candidate).then_some(candidate)
                })?;
                entry.insert(slot);
            }
        }
        self.by_id = slots;
        self.next = next;
        self.revision = Some(table.revision());
        Some(())
    }

    fn follow(slots: &mut BTreeMap<ObjectId, Slot>, changes: &[Change], viewer: Seat) {
        for pair in changes.windows(2) {
            // In the forward direction, only the seat that knows the hand
            // entry can follow it. On undo, that same seat can follow it back.
            // Shuffles have an empty knowledge list on the fresh-ID side.
            let (from, to, knows_hand) = match pair {
                [
                    Change::ResetKnowledge {
                        id,
                        edit,
                        forget: true,
                    },
                    Change::ReplaceId { from, to },
                ] if id == from => (from, to, edit.after.contains(&viewer)),
                [
                    Change::ReplaceId { from, to },
                    Change::ResetKnowledge {
                        id,
                        edit,
                        forget: false,
                    },
                ] if id == to => (from, to, edit.before.contains(&viewer)),
                _ => continue,
            };
            if let Some(slot) = slots.remove(from)
                && knows_hand
            {
                slots.insert(*to, slot);
            }
        }
    }
}
