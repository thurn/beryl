//! Host-side history, turn snapshots and the projection of entries into log lines.

use std::collections::{BTreeMap, BTreeSet};

use crate::apply::{commit, next_entry, perform, perform_history};
use crate::change::Edit;
use crate::state::ObjectId;
use crate::{Action, Chance, Change, Entry, Refusal, Seat, Table, apply, permissions, undo};

mod line;
mod touches;

pub(crate) use line::Summary;
pub use line::{ConnectionEvent, connection_line, line};
pub use touches::Touches;

/// The history belongs to the host. The adapter sends projected entries and
/// strike status to its viewers.
#[derive(Clone, Debug)]
pub struct Log {
    entries: Vec<Entry>,
    snapshots: BTreeMap<u32, Table>,
    /// Strike status changes only when a rewind is applied or undone.
    strikes: BTreeMap<u32, BTreeSet<u32>>,
    /// Shuffling and entering a hand assign a fresh id to the same card.
    prior_ids: BTreeMap<ObjectId, ObjectId>,
}

impl Log {
    pub fn new(initial: &Table) -> Self {
        Self {
            entries: Vec::new(),
            snapshots: BTreeMap::from([(initial.entry_number, initial.clone())]),
            strikes: BTreeMap::new(),
            prior_ids: BTreeMap::new(),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn is_struck(&self, n: u32) -> bool {
        self.struck_at(self.tip())
            .is_some_and(|set| set.contains(&n))
    }

    pub(crate) fn tip(&self) -> u32 {
        self.entries
            .last()
            .map_or_else(|| self.base(), |entry| entry.n)
    }

    fn base(&self) -> u32 {
        *self
            .snapshots
            .first_key_value()
            .expect("initial snapshot")
            .0
    }

    pub(crate) fn get(&self, n: u32) -> Option<&Entry> {
        let index = n.checked_sub(self.base())?.checked_sub(1)?;
        self.entries.get(index as usize)
    }

    fn struck_at(&self, n: u32) -> Option<&BTreeSet<u32>> {
        self.strikes.range(..=n).next_back().map(|(_, set)| set)
    }

    pub(crate) fn original_id(&self, mut id: ObjectId) -> ObjectId {
        while let Some(&prior) = self.prior_ids.get(&id) {
            id = prior;
        }
        id
    }

    /// Applies and records one action. The same table must be used for every
    /// action so its entry numbers and this log stay in sync.
    pub fn apply(
        &mut self,
        table: &mut Table,
        by: Seat,
        action: &Action,
        chance: &mut Chance,
    ) -> Result<&Entry, Refusal> {
        if table.entry_number != self.tip() {
            return Err(Refusal::INVALID);
        }
        let turn = (table.game_match.in_game, table.turn);
        let entry = self.execute(table, by, action, chance)?;
        self.record(table, turn, entry);
        Ok(self.entries.last().expect("appended entry"))
    }

    fn execute(
        &self,
        table: &mut Table,
        by: Seat,
        action: &Action,
        chance: &mut Chance,
    ) -> Result<Entry, Refusal> {
        match *action {
            Action::Undo { entry } => {
                permissions::check(table, by, action)?;
                let n = next_entry(table)?;
                let target = undo::target(self, by, entry)?;
                let (changes, seen) = undo::inverse(self, table, target);
                let (changes, touches) = perform_history(table, action, changes);
                let mut result = commit(table, n, by, action, changes, touches);
                result.undoes = Some(target.n);
                result.seen = seen;
                result.undone_action = Some(target.action.clone());
                result.summary = target.summary.clone();
                Ok(result)
            }
            Action::Rewind { to } => {
                permissions::check(table, by, action)?;
                let n = next_entry(table)?;
                let mut after = rebuild(self, to)?;
                undo::keep_knowledge(self, table, &mut after);
                let changes = vec![Change::RestoreTable(Box::new(Edit {
                    before: table.clone(),
                    after,
                }))];
                let (changes, touches) = perform_history(table, action, changes);
                Ok(commit(table, n, by, action, changes, touches))
            }
            _ => apply(table, by, action, chance),
        }
    }

    fn record(&mut self, table: &Table, turn: (bool, u32), entry: Entry) {
        if let Action::Rewind { to } = entry.action {
            let mut struck = self.struck_at(to).cloned().unwrap_or_default();
            struck.extend(to + 1..entry.n);
            self.strikes.insert(entry.n, struck);
        } else if let Some(target) = entry.undoes.and_then(|n| self.get(n)) {
            if matches!(target.action, Action::Rewind { .. }) {
                let struck_before = self.struck_at(target.n - 1).cloned().unwrap_or_default();
                self.strikes.insert(entry.n, struck_before);
            }
        } else {
            for change in &entry.changes {
                if let Change::ReplaceId { from, to } = change {
                    self.prior_ids.insert(*to, *from);
                }
            }
        }

        if turn != (table.game_match.in_game, table.turn) {
            self.snapshots.insert(entry.n, table.clone());
        }
        self.entries.push(entry);
    }
}

/// Reconstructs the table immediately after the numbered entry. Number zero is
/// the initial table for a new log. A struck entry is still a valid destination.
pub fn rebuild(log: &Log, upto: u32) -> Result<Table, Refusal> {
    if upto > log.tip() {
        return Err(Refusal::INVALID);
    }
    let (&start, snapshot) = log
        .snapshots
        .range(..=upto)
        .next_back()
        .ok_or(Refusal::INVALID)?;
    let mut table = snapshot.clone();
    for n in (start..upto).map(|n| n + 1) {
        let entry = log.get(n).ok_or(Refusal::INVALID)?;
        if matches!(entry.action, Action::Undo { .. } | Action::Rewind { .. }) {
            // These changes were derived from the prefix when the entry was
            // committed. Reusing them avoids recursively replaying each prefix.
            perform(&mut table, &entry.changes, &entry.action);
            table.entry_number = n;
        } else {
            let mut chance = Chance::replaying(outcomes(&entry.changes).collect());
            apply(&mut table, entry.by, &entry.action, &mut chance)?;
        }
    }
    Ok(table)
}

pub(crate) fn outcomes(changes: &[Change]) -> impl Iterator<Item = u32> + '_ {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Random { outcomes } | Change::Roll { outcomes, .. } => {
                Some(outcomes.as_slice())
            }
            _ => None,
        })
        .flatten()
        .copied()
}
