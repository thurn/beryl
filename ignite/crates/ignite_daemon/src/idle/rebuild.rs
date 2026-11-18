//! Rebuild counts: the instructions the compiler runs to rebuild after one fixed edit, in a
//! landing tree that has built the verified commit. A count is an instrument: it files nothing
//! and blocks nothing.
//!
//! Each `[[rebuild]]` edit in `ignite.toml` names a file and the text to append to it. A
//! measurement builds the edit's products, saves the file's contents, appends the text, builds
//! the products again and sums the compiler wrapper's units, then writes the saved contents back.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use ignite_core::ids::{Commit, TreeId};
use ignite_core::manifest::RebuildEdit;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{CompileUnit, Record};
use ignite_core::time::Timestamp;
use ignite_core::traits::{BuildResult, Error, Result};

use super::state::Journal;
use super::{Idle, REBUILD_EVERY};
use crate::server::Context;

/// Counts kept per edit; older ones are dropped.
const KEEP_COUNTS: usize = 20;

/// Crates a rendered count names before it sums up the rest.
const SHOWN_CRATES: usize = 3;

/// One measurement of a rebuild edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RebuildCount {
    pub name: String,
    /// The verified commit the tree had built before the edit.
    pub commit: Commit,
    pub at: Timestamp,
    /// Instructions the compiler ran for the rebuild, over every crate.
    pub instructions: f64,
    /// Each recompiled crate's instructions, largest first.
    pub crates: Vec<(String, f64)>,
}

impl RebuildCount {
    fn from_units(name: &str, commit: Commit, at: Timestamp, units: &[CompileUnit]) -> Self {
        let mut crates: Vec<(String, f64)> = Vec::new();
        for unit in units {
            let instructions = unit.instructions.unwrap_or(0.0);
            match crates.iter_mut().find(|(c, _)| *c == unit.crate_name) {
                Some((_, sum)) => *sum += instructions,
                None => crates.push((unit.crate_name.clone(), instructions)),
            }
        }
        crates.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        RebuildCount {
            name: name.to_owned(),
            commit,
            at,
            instructions: crates.iter().map(|(_, n)| n).sum(),
            crates,
        }
    }

    /// The count as `ignite rebuild` prints it: the total, then the largest crates.
    pub fn render(&self) -> String {
        let mut text = format!(
            "{}  {}  {} crates  at {}\n",
            self.name,
            billions(self.instructions),
            self.crates.len(),
            self.commit
        );
        let rest = self.crates.len().saturating_sub(SHOWN_CRATES);
        for (index, (name, instructions)) in self.crates.iter().take(SHOWN_CRATES).enumerate() {
            let _ = write!(text, "  {name:<20} {}", billions(*instructions));
            if index + 1 == SHOWN_CRATES && rest > 0 {
                let _ = write!(text, "   (+{rest} more)");
            }
            text.push('\n');
        }
        text
    }
}

fn billions(instructions: f64) -> String {
    let billions = instructions / 1e9;
    if billions >= 10.0 {
        format!("{billions:.0} billion")
    } else {
        format!("{billions:.1} billion")
    }
}

/// `ignite rebuild --list`: each edit and what it does.
pub(super) fn list(edits: &[RebuildEdit]) -> String {
    if edits.is_empty() {
        return "ignite.toml names no rebuild edit\n".to_owned();
    }
    let mut text = String::new();
    for edit in edits {
        let _ = writeln!(text, "{:<16} {}", edit.name, edit.edit);
    }
    text
}

impl Idle {
    /// The first edit due a measurement: never tried, or tried at a commit the verified commit
    /// has since moved [`REBUILD_EVERY`] commits past.
    pub(super) fn rebuild_due(&self, ctx: &Context) -> Option<String> {
        if ctx.config.rebuild.is_empty() {
            return None;
        }
        let verified = Self::verified(ctx).ok()?;
        let tried = self.state().rebuild_tried.clone();
        ctx.config
            .rebuild
            .iter()
            .find(|edit| match tried.get(&edit.name) {
                None => true,
                Some(commit) => {
                    *commit != verified
                        && self
                            .host
                            .commits_between(commit, &verified)
                            .is_ok_and(|n| n >= REBUILD_EVERY)
                }
            })
            .map(|edit| edit.name.clone())
    }

    /// Measures the edit `name` at the verified commit and stores the count. `None` when every
    /// landing tree is busy.
    pub(super) fn rebuild(&self, ctx: &Context, name: &str) -> Result<Option<RebuildCount>> {
        self.undo_unfinished()?;
        let edit = ctx
            .config
            .rebuild
            .iter()
            .find(|edit| edit.name == name)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("no rebuild edit is named {name}")))?;
        let commit = Self::verified(ctx)?;
        let Some((tree, path)) = self.host.lease(&commit)? else {
            return Ok(None);
        };
        let tried = self.update(|state| {
            state.rebuild_tried.insert(name.to_owned(), commit.clone());
        });
        let measured = tried.and_then(|()| self.measure(ctx, &edit, &tree, &path));
        // The tree's builds now match the edited file, which no commit holds.
        let released = self.host.release(&tree, None);
        let units = measured?;
        released?;
        let count = RebuildCount::from_units(name, commit, self.clock.now(), &units);
        self.update(|state| {
            state.counts.push(count.clone());
            let of_name = state.counts.iter().filter(|c| c.name == name).count();
            let mut drop = of_name.saturating_sub(KEEP_COUNTS);
            state.counts.retain(|c| {
                let dropped = drop > 0 && c.name == name;
                drop -= usize::from(dropped);
                !dropped
            });
        })?;
        // A count that does not reach the hub is still stored here.
        let _ = ctx.hub.records(&[self.record(ctx, &tree, &count, units)]);
        Ok(Some(count))
    }

    /// Builds the products, applies the edit, builds them again and undoes the edit. Returns
    /// the compiler units of the second build.
    fn measure(
        &self,
        ctx: &Context,
        edit: &RebuildEdit,
        tree: &TreeId,
        path: &Path,
    ) -> Result<Vec<CompileUnit>> {
        for product in &edit.products {
            if let BuildResult::Failed { output, .. } = ctx.builds.ensure(tree, product, &[]) {
                return Err(Error::Io(format!(
                    "{product} failed to build before the edit; its output is in {}",
                    output.display()
                )));
            }
        }
        let file = path.join(&edit.file);
        let original = fs::read_to_string(&file)?;
        self.update(|state| {
            state.journal = Some(Journal {
                name: edit.name.clone(),
                tree: tree.clone(),
                file: file.clone(),
                original: original.clone(),
            });
        })?;
        let applied = fs::write(&file, format!("{original}{}", edit.append));
        let mut units = Vec::new();
        let mut failed = None;
        if applied.is_ok() {
            for product in &edit.products {
                match ctx.builds.ensure(tree, product, &[]) {
                    BuildResult::Current => {}
                    BuildResult::Built { units: built, .. } => units.extend(built),
                    BuildResult::Failed { output, .. } => {
                        failed = Some((product, output));
                        break;
                    }
                }
            }
        }
        self.restore()?;
        applied?;
        if let Some((product, output)) = failed {
            return Err(Error::Io(format!(
                "{product} failed to build after the edit; its output is in {}",
                output.display()
            )));
        }
        Ok(units)
    }

    /// Writes back the file of an edit that a stop left applied, and frees its tree.
    pub(super) fn undo_unfinished(&self) -> Result<()> {
        if let Some(tree) = self.restore()? {
            // The lease may not have outlived the stop.
            let _ = self.host.release(&tree, None);
        }
        Ok(())
    }

    /// Writes back the saved contents of an applied edit, if one is saved, and returns its
    /// tree.
    fn restore(&self) -> Result<Option<TreeId>> {
        let Some(journal) = self.state().journal.clone() else {
            return Ok(None);
        };
        fs::write(&journal.file, &journal.original)?;
        self.update(|state| state.journal = None)?;
        Ok(Some(journal.tree))
    }

    /// The stored counts: each edit's newest, or every stored count of `name`, newest first.
    pub fn last_counts(&self, name: Option<&str>) -> String {
        let state = self.state();
        let mut shown: Vec<&RebuildCount> = Vec::new();
        for count in state.counts.iter().rev() {
            let wanted = match name {
                Some(name) => count.name == name,
                None => !shown.iter().any(|c| c.name == count.name),
            };
            if wanted {
                shown.push(count);
            }
        }
        if shown.is_empty() {
            return "no rebuild count is stored yet\n".to_owned();
        }
        shown.iter().map(|c| c.render()).collect()
    }

    fn record(
        &self,
        ctx: &Context,
        tree: &TreeId,
        count: &RebuildCount,
        units: Vec<CompileUnit>,
    ) -> Record {
        Record {
            machine: ctx.machine.clone(),
            ticket: None,
            session: None,
            tree: Some(tree.clone()),
            verb: "rebuild".to_owned(),
            class: Class::Idle,
            family: Some(count.name.clone()),
            queued_s: 0.0,
            wall_s: 0.0,
            cpu_s: units.iter().map(|u| u.cpu_s).sum(),
            instructions: Some(count.instructions),
            peak_mb: 0,
            outcome: None,
            commit: Some(count.commit.clone()),
            ignite: self.settings.ignite.clone(),
            at: count.at,
            compile_units: units,
        }
    }
}
