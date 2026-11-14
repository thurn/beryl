use std::collections::{BTreeMap, BTreeSet};

use crate::apply::{AppliedChanges, Presentation};
use crate::change::TableEdit;
use crate::log::UndoControl;
use crate::state::Zone;
use crate::{Action, Change, Entry, Log, Refusal, Seat, Table};

pub(crate) struct Targets<'a> {
    live: Vec<&'a Entry>,
}

impl<'a> Targets<'a> {
    pub(crate) fn new(log: &'a Log) -> Self {
        let strikes = log.struck_at(log.tip());
        let unstruck = log
            .entries()
            .iter()
            .filter(|entry| !strikes.is_some_and(|strikes| strikes.contains(&entry.n)));
        let undone: BTreeSet<_> = unstruck.clone().filter_map(|entry| entry.undoes).collect();
        let live = unstruck
            .filter(|entry| entry.undoes.is_none() && !undone.contains(&entry.n))
            .collect();
        Self { live }
    }

    pub(crate) fn last_draw(&self, by: Seat) -> Option<u32> {
        self.live
            .iter()
            .rev()
            .find(|entry| own_draw(entry, by))
            .map(|entry| entry.n)
    }

    fn named(&self, by: Seat, n: u32) -> Option<&'a Entry> {
        let index = self.live.binary_search_by_key(&n, |entry| entry.n).ok()?;
        let entry = self.live[index];
        (matches!(entry.action, Action::Batch { .. }) || own_draw(entry, by)).then_some(entry)
    }

    pub(crate) fn target(&self, by: Seat, requested: Option<u32>) -> Result<&'a Entry, Refusal> {
        let entry = match requested {
            Some(n) => self.named(by, n),
            None => self.live.iter().rev().copied().find(|entry| entry.by == by),
        }
        .ok_or(Refusal::INVALID)?;
        match self.control(entry) {
            UndoControl::Available => Ok(entry),
            UndoControl::Blocked(blocking) => Err(Refusal::Invalid { blocking }),
        }
    }

    pub(crate) fn batch_control(&self, by: Seat, entry: &Entry) -> Option<UndoControl> {
        if !matches!(entry.action, Action::Batch { .. }) {
            return None;
        }
        self.named(by, entry.n).map(|target| self.control(target))
    }

    fn control(&self, entry: &Entry) -> UndoControl {
        let first_later = self.live.partition_point(|later| later.n <= entry.n);
        let blocking: Vec<_> = self.live[first_later..]
            .iter()
            .filter(|later| entry.touches.conflicts(&later.touches))
            .map(|later| later.n)
            .collect();
        if blocking.is_empty() {
            UndoControl::Available
        } else {
            UndoControl::Blocked(blocking)
        }
    }
}

fn own_draw(entry: &Entry, by: Seat) -> bool {
    matches!(entry.action, Action::Draw { .. }) && entry.by == by
}

pub(crate) fn inverse(log: &Log, table: &Table, entry: &Entry) -> (AppliedChanges, bool) {
    let mut seen = false;
    let mut applied = AppliedChanges::default();
    let members: Vec<_> = entry.member_changes().collect();
    for (presentation, forward) in members.into_iter().rev() {
        let mut changes = Vec::new();
        for change in forward.iter().rev() {
            match change.inverse() {
                Change::Nothing => {}
                Change::RestoreTable(mut edit) => {
                    keep_knowledge(log, table, &mut edit.after);
                    edit.before.clone_from(table);
                    changes.push(Change::RestoreTable(edit));
                }
                inverse => changes.push(inverse),
            }
            // Returning a draw restores its library position and retains what
            // the players learned. Keep that distinction in the undo line.
            if matches!(change, Change::Move { from, to, .. }
                if matches!(from.zone, Zone::Library(_)) && matches!(to.zone, Zone::Hand(_)))
            {
                seen = true;
            }
        }
        let presentation = match presentation {
            Some(
                Presentation::Shuffle { .. }
                | Presentation::MulliganShuffle { .. }
                | Presentation::Reset
                | Presentation::GameReset
                | Presentation::LobbyReset,
            ) => presentation.cloned(),
            Some(Presentation::Phase(_)) => changes.iter().find_map(|change| match change {
                Change::SetTable(TableEdit::SetPhase { edit }) => {
                    Some(Presentation::Phase(edit.after))
                }
                _ => None,
            }),
            _ => None,
        };
        applied.record(changes, presentation);
    }
    (applied, seen)
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
