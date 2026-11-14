use std::collections::{BTreeMap, BTreeSet};

use crate::state::Zone;
use crate::{Action, Change, Entry, Log, Refusal, Seat, Table};

pub(crate) fn target(log: &Log, by: Seat, requested: Option<u32>) -> Result<&Entry, Refusal> {
    let undone: BTreeSet<_> = log
        .entries()
        .iter()
        .filter(|entry| !log.is_struck(entry.n))
        .filter_map(|entry| entry.undoes)
        .collect();
    let live = |entry: &&Entry| {
        !log.is_struck(entry.n) && !undone.contains(&entry.n) && entry.undoes.is_none()
    };
    let entry = if let Some(n) = requested {
        log.get(n)
            .filter(live)
            .filter(|entry| matches!(entry.action, Action::Batch { .. }))
    } else {
        log.entries()
            .iter()
            .rev()
            .filter(live)
            .find(|entry| entry.by == by)
    }
    .ok_or(Refusal::INVALID)?;
    let blocking: Vec<_> = log
        .entries()
        .iter()
        .filter(live)
        .filter(|later| later.n > entry.n && entry.touches.conflicts(&later.touches))
        .map(|later| later.n)
        .collect();
    if blocking.is_empty() {
        Ok(entry)
    } else {
        Err(Refusal::Invalid { blocking })
    }
}

pub(crate) fn inverse(log: &Log, table: &Table, entry: &Entry) -> (Vec<Change>, bool) {
    let mut seen = false;
    let mut changes = Vec::new();
    for change in entry.changes.iter().rev() {
        match change.inverse() {
            Change::Nothing => {}
            Change::RestoreTable(mut edit) => {
                keep_knowledge(log, table, &mut edit.after);
                edit.before.clone_from(table);
                changes.push(Change::RestoreTable(edit));
            }
            inverse => changes.push(inverse),
        }
        // The resulting card remains known even when its old id and library
        // position are restored. Keep that distinction in the undo line.
        if matches!(change, Change::Move { from, to, .. }
            if matches!(from.zone, Zone::Library(_)) && matches!(to.zone, Zone::Hand(_)))
        {
            seen = true;
        }
    }
    (changes, seen)
}

pub(crate) fn keep_knowledge(log: &Log, current: &Table, restored: &mut Table) {
    let mut known: BTreeMap<_, _> = current
        .objects
        .values()
        .map(|object| (log.original_id(object.id), &object.known_to))
        .collect();
    // A rewind can restore a token or a card removed when a game ended.
    // In that case use its last recorded knowledge before it left the table.
    let mut missing: BTreeSet<_> = restored
        .objects
        .keys()
        .map(|&id| log.original_id(id))
        .filter(|id| !known.contains_key(id))
        .collect();
    for change in log
        .entries()
        .iter()
        .rev()
        .flat_map(|entry| entry.changes.iter().rev())
    {
        if missing.is_empty() {
            break;
        }
        match change {
            Change::Remove { object, .. } => {
                let id = log.original_id(object.id);
                if missing.remove(&id) {
                    known.insert(id, &object.known_to);
                }
            }
            Change::RestoreTable(edit) => {
                for object in edit.before.objects.values() {
                    let id = log.original_id(object.id);
                    if missing.remove(&id) {
                        known.insert(id, &object.known_to);
                    }
                }
            }
            _ => {}
        }
    }
    for object in restored.objects.values_mut() {
        if let Some(seats) = known.get(&log.original_id(object.id)) {
            for &seat in *seats {
                if !object.known_to.contains(&seat) {
                    object.known_to.push(seat);
                }
            }
        }
    }
}
