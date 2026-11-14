//! Host-side history, turn snapshots and the projection of entries into log lines.

use std::collections::{BTreeMap, BTreeSet};

use crate::apply::{AppliedChanges, Presentation, commit, next_entry, perform, perform_history};
use crate::change::Edit;
use crate::state::ObjectId;
use crate::{Action, Chance, Change, Entry, Refusal, Seat, Table, apply, permissions, undo};

mod line;
mod touches;

pub(crate) use line::Summary;
pub use line::{ConnectionEvent, connection_line, line};
pub use touches::Touches;

/// An offered row-level undo and the entry numbers that would block it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoControl {
    Available,
    Blocked(Vec<u32>),
}

/// One row of a seat's history. The actor is a seat id so the adapter can
/// supply the corresponding display name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryLine<'a> {
    pub n: u32,
    pub by: Seat,
    pub text: &'a str,
    pub struck: bool,
    pub undo: Option<UndoControl>,
}

/// The authoritative history belongs to the host. Use [`Self::history`]
/// to build a seat's log view.
#[derive(Clone, Debug)]
pub struct Log {
    entries: Vec<Entry>,
    /// Per-entry wording in seat order, captured when the action happened.
    lines: Vec<Vec<String>>,
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
            lines: Vec::new(),
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

    /// Projects the complete log from recorded wording and current strike and
    /// undo state. Row-level undo is offered for live batches. Draws use the
    /// separate [`Self::last_draw`] shortcut.
    pub fn history(&self, table: &Table, viewer: Seat) -> impl Iterator<Item = HistoryLine<'_>> {
        let targets = self
            .undo_ready(table, viewer)
            .ok()
            .map(|()| undo::Targets::new(self));
        let strikes = self.struck_at(self.tip());
        self.entries
            .iter()
            .zip(&self.lines)
            .filter_map(move |(entry, lines)| {
                Some(HistoryLine {
                    n: entry.n,
                    by: entry.by,
                    text: lines.get(usize::from(viewer.0))?,
                    struck: strikes.is_some_and(|strikes| strikes.contains(&entry.n)),
                    undo: targets
                        .as_ref()
                        .and_then(|targets| targets.batch_control(viewer, entry)),
                })
            })
    }

    /// Returns the entry that `Action::Undo { entry }` would undo, or the
    /// same refusal applying the action would return.
    pub fn undo_target(&self, table: &Table, by: Seat, entry: Option<u32>) -> Result<u32, Refusal> {
        Ok(self.resolve_undo(table, by, entry)?.n)
    }

    /// Finds the seat's latest live draw, even if it is currently blocked.
    /// Pass its number to [`Self::undo_target`] or to `Action::Undo`.
    pub fn last_draw(&self, table: &Table, by: Seat) -> Option<u32> {
        self.undo_ready(table, by).ok()?;
        undo::Targets::new(self).last_draw(by)
    }

    fn undo_ready(&self, table: &Table, by: Seat) -> Result<(), Refusal> {
        if table.entry_number != self.tip() {
            return Err(Refusal::INVALID);
        }
        permissions::check_actor(table, by)?;
        next_entry(table)?;
        Ok(())
    }

    fn resolve_undo(&self, table: &Table, by: Seat, entry: Option<u32>) -> Result<&Entry, Refusal> {
        self.undo_ready(table, by)?;
        undo::Targets::new(self).target(by, entry)
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
        self.entries.get(self.index(n)?)
    }

    fn index(&self, n: u32) -> Option<usize> {
        Some(n.checked_sub(self.base())?.checked_sub(1)? as usize)
    }

    pub(crate) fn struck_at(&self, n: u32) -> Option<&BTreeSet<u32>> {
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
        let before = table.clone();
        let entry = self.execute(table, by, action, chance)?;
        self.record(&before, table, entry);
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
                let target = self.resolve_undo(table, by, entry)?;
                let n = next_entry(table)?;
                let (applied, seen) = undo::inverse(self, table, target);
                let (applied, touches) = perform_history(table, action, applied);
                let mut result = commit(table, n, by, action, applied, touches);
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
                let mut applied = AppliedChanges::default();
                applied.record(
                    vec![Change::RestoreTable(Box::new(Edit {
                        before: table.clone(),
                        after,
                    }))],
                    Some(Presentation::Reset),
                );
                let (applied, touches) = perform_history(table, action, applied);
                Ok(commit(table, n, by, action, applied, touches))
            }
            _ => apply(table, by, action, chance),
        }
    }

    fn record(&mut self, before: &Table, table: &Table, entry: Entry) {
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

        if (before.game_match.in_game, before.turn) != (table.game_match.in_game, table.turn) {
            self.snapshots.insert(entry.n, table.clone());
        }
        let lines = if let Some(target) = entry.undoes {
            let index = self.index(target).expect("recorded undo target");
            self.lines[index]
                .iter()
                .map(|text| line::undo_line(text, entry.seen))
                .collect()
        } else {
            table
                .seats()
                .map(|viewer| line::recorded_line(&entry, before, table, viewer))
                .collect()
        };
        self.lines.push(lines);
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
