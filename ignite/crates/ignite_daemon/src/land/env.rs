//! What a landing needs besides the traits in [`Context`]: the machine's build trees
//! ([`LandTrees`]) and the gate ([`LandGate`]), with their real implementations over
//! [`Trees`] and [`Checks`]. Tests replace both with fakes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, Handle, SessionId, TicketId, TreeId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{Verdict, VerdictKey};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_git::{push, refs, save};

use crate::checks::{Carried, CheckRun, Checks, Excused, Failure, GateRun, RunOptions};
use crate::server::Context;
use crate::trees::{self, Trees};

/// A ticket tree and who holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketTree {
    pub tree: TreeId,
    pub path: PathBuf,
    pub ticket: TicketId,
    pub session: Option<SessionId>,
    /// The ticket's branch, which the tree must have checked out.
    pub branch: Option<String>,
}

/// The tree a gate runs in: the ticket tree, or a landing tree checked out at the rebased
/// commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateTree {
    pub tree: TreeId,
    pub path: PathBuf,
}

/// The build trees, as a landing uses them.
pub trait LandTrees: Send + Sync {
    /// The ticket tree `tree` and its ticket; `None` when no ticket holds it.
    fn ticket_tree(&self, tree: &TreeId) -> Result<Option<TicketTree>>;
    /// The branch checked out in `path`; `None` for a detached `HEAD`.
    fn branch(&self, path: &Path) -> Result<Option<String>>;
    /// The commit checked out in `path`.
    fn head(&self, path: &Path) -> Result<Commit>;
    /// The files in `path` that are neither committed nor ignored.
    fn uncommitted(&self, path: &Path) -> Result<Vec<String>>;
    /// The commits after `from` up to `to`, oldest first.
    fn commits(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>>;
    /// The subject lines of the commits after `from` up to `to`, oldest first.
    fn subjects(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<String>>;
    /// Whether `ancestor` is in the history of `descendant`, as `path`'s repository sees it.
    fn is_ancestor(&self, path: &Path, ancestor: &Commit, descendant: &Commit) -> Result<bool>;
    /// Leases a free landing tree checked out at `at`; `None` when every one is busy. Asked
    /// again by a landing that holds a tree, it returns that tree checked out at `at`, so the
    /// landing's next pass builds only what changed since its last.
    fn lease_landing(&self, holder: &str, at: &Commit) -> Result<Option<GateTree>>;
    /// Frees a landing tree; `built_at` is the commit its builds now match.
    fn release_landing(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()>;
    /// Whether a landing that changes `change` builds less at `head` in its ticket tree, whose
    /// build matches `built`, than in any free landing tree (see [`Trees::ticket_builds_less`]).
    /// Never when `built` is unknown.
    fn ticket_builds_less(
        &self,
        _ticket: &TreeId,
        _built: Option<&Commit>,
        _head: &Commit,
        _change: &[String],
    ) -> Result<bool> {
        Ok(false)
    }
    /// Frees a ticket tree after its landing, parked at `park_at`. `caller` is the client's
    /// process, which is spared with its ancestors when the tree's processes are ended.
    fn release_ticket(&self, tree: &TreeId, park_at: &Commit, caller: Option<u32>) -> Result<()>;
    /// Deletes the remote branch that held the ticket's work saved at a release.
    fn delete_saved(&self, ticket: &TicketId) -> Result<()>;
}

/// What one check did in [`LandGate::run_checks`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckVerdict {
    pub check: CheckName,
    pub outcome: Outcome,
    /// The operating system the check's result is keyed to, for a check that is not portable.
    pub platform: Option<Os>,
    /// What a failure said: where, its message and the end of its output.
    pub output: String,
}

/// The landing gate.
pub trait LandGate: Send + Sync {
    /// Runs the always set and the gate in `tree` as it is, judging again only the families
    /// `carried` does not carry over from an earlier pass that passed, and leaving out the
    /// checks in `excused` as known to fail.
    fn run(
        &self,
        ctx: &Context,
        tree: &GateTree,
        options: &RunOptions,
        carried: Option<&Carried>,
        excused: &Excused,
    ) -> GateRun;
    /// Runs `checks` in `tree` as it is, each from the result cache where it can be, with the
    /// immediate rerun of a failure. A check the manifests no longer declare has no verdict.
    fn run_checks(
        &self,
        ctx: &Context,
        tree: &GateTree,
        checks: &[CheckName],
        options: &RunOptions,
    ) -> Vec<CheckVerdict>;
    /// Runs every check of one family in `tree` as it is, each from the result cache where it
    /// can be. A family the manifests do not declare is incomplete.
    fn run_family(
        &self,
        ctx: &Context,
        tree: &GateTree,
        family: &str,
        options: &RunOptions,
    ) -> CheckRun;
    /// The verdict stored under `key`: the hub's, or with the default body only the hub's; the
    /// machine's gate also looks in its local database (see [`crate::verdicts::find`]).
    fn verdict(&self, ctx: &Context, key: &VerdictKey) -> Option<Verdict> {
        let found = ctx.hub.verdict(key).ok().flatten();
        found.filter(|verdict| verdict.key == *key)
    }
    /// Stores the verdict that `gated`, a gate that passed in `tree` at `head`, earned under
    /// `key`: at the hub, and for the machine's gate in its local database as well.
    fn remember(
        &self,
        ctx: &Context,
        tree: &GateTree,
        key: VerdictKey,
        head: Commit,
        gated: &GateRun,
    ) -> Result<()> {
        let _ = tree;
        ctx.hub.store_verdict(&Verdict {
            key,
            head,
            machine: ctx.machine.as_str().to_owned(),
            passed_at: ctx.clock.now(),
            families: gated.scoped.clone(),
            left_to_tip: u32::try_from(gated.left).unwrap_or(u32::MAX),
            always_not_run: Vec::new(),
        })
    }
    /// When the landing's handle began. A daemon restart that cuts a landing off runs it again
    /// under the same handle, so this is earlier than the run's own start when one did.
    fn began(&self, handle: &Handle) -> Option<Timestamp> {
        let _ = handle;
        None
    }
}

/// [`LandTrees`] over the machine's [`Trees`] and `git`.
#[derive(Clone, Debug)]
pub struct MachineTrees {
    pub trees: Trees,
}

impl LandTrees for MachineTrees {
    fn ticket_tree(&self, tree: &TreeId) -> Result<Option<TicketTree>> {
        let status = self.trees.status()?;
        let Some(row) = status
            .trees
            .into_iter()
            .find(|row| row.tree == *tree && row.kind == trees::Kind::Ticket)
        else {
            return Ok(None);
        };
        if row.state != "leased" {
            return Ok(None);
        }
        let Some(ticket) = row.ticket else {
            return Ok(None);
        };
        let invalid = |e: ignite_core::ids::IdError| Error::Invalid(e.to_string());
        Ok(Some(TicketTree {
            tree: row.tree,
            path: row.path,
            ticket: TicketId::new(ticket).map_err(invalid)?,
            session: row
                .session
                .map(SessionId::new)
                .transpose()
                .map_err(invalid)?,
            branch: row.branch,
        }))
    }

    fn branch(&self, path: &Path) -> Result<Option<String>> {
        refs::branch(path)
    }

    fn head(&self, path: &Path) -> Result<Commit> {
        refs::head(path)
    }

    fn uncommitted(&self, path: &Path) -> Result<Vec<String>> {
        save::uncommitted(path)
    }

    fn commits(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
        refs::commits_between(path, from, to)
    }

    fn subjects(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<String>> {
        refs::subjects_between(path, from, to)
    }

    fn is_ancestor(&self, path: &Path, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        refs::is_ancestor(path, ancestor, descendant)
    }

    /// Leases the tree under the handle of the verb asking, when it has one, so that a restarted
    /// daemon keeps the tree for that verb when it runs it again (see [`Trees::lease_landing_for`]).
    fn lease_landing(&self, holder: &str, at: &Commit) -> Result<Option<GateTree>> {
        let owner = crate::steps::current().and_then(|activity| activity.handle());
        let lease = match &owner {
            Some(owner) => self.trees.lease_landing_for(holder, owner, at)?,
            None => self.trees.lease_landing(holder, at)?,
        };
        Ok(lease.map(|lease| GateTree {
            tree: lease.tree,
            path: lease.path,
        }))
    }

    fn release_landing(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        self.trees.release_landing(tree, built_at)
    }

    fn ticket_builds_less(
        &self,
        ticket: &TreeId,
        built: Option<&Commit>,
        head: &Commit,
        change: &[String],
    ) -> Result<bool> {
        match built {
            Some(built) => self.trees.ticket_builds_less(ticket, built, head, change),
            None => Ok(false),
        }
    }

    /// A landed tree has no work left to save (the landing refused stray files), so it is reset
    /// without the save that [`Trees::release`] pushes to the ticket's saved branch.
    fn release_ticket(&self, tree: &TreeId, park_at: &Commit, caller: Option<u32>) -> Result<()> {
        self.trees
            .release_unsaved(tree, park_at, caller)
            .map(|_| ())
    }

    fn delete_saved(&self, ticket: &TicketId) -> Result<()> {
        let config = self.trees.config();
        push::delete(&config.repo, &config.remote, &trees::save_branch(ticket))
    }
}

/// [`LandGate`] over the check runner.
pub struct ChecksGate {
    pub checks: Arc<Checks>,
}

impl LandGate for ChecksGate {
    fn run(
        &self,
        ctx: &Context,
        tree: &GateTree,
        options: &RunOptions,
        carried: Option<&Carried>,
        excused: &Excused,
    ) -> GateRun {
        self.checks
            .gate_carrying(ctx, &tree.tree, options, carried, excused)
    }

    fn began(&self, handle: &Handle) -> Option<Timestamp> {
        self.checks.handle_began(handle)
    }

    fn verdict(&self, ctx: &Context, key: &VerdictKey) -> Option<Verdict> {
        crate::checks::find_verdict(ctx, &self.checks, key)
    }

    fn remember(
        &self,
        ctx: &Context,
        tree: &GateTree,
        key: VerdictKey,
        head: Commit,
        gated: &GateRun,
    ) -> Result<()> {
        let (runner, open) = self.checks.open(ctx, &tree.tree)?;
        crate::checks::store_verdict(ctx, &runner, &open, gated, key, head)
    }

    fn run_checks(
        &self,
        ctx: &Context,
        tree: &GateTree,
        checks: &[CheckName],
        options: &RunOptions,
    ) -> Vec<CheckVerdict> {
        let Ok((runner, open)) = self.checks.open(ctx, &tree.tree) else {
            return Vec::new();
        };
        let mut families: Vec<&str> = checks.iter().map(CheckName::family).collect();
        families.sort_unstable();
        families.dedup();
        let mut run = CheckRun::default();
        let mut prepared = runner.prepare(ctx, &open, &families, options, &mut run);
        let mut platforms = BTreeMap::new();
        for listed in &mut prepared {
            listed.keep(|check| checks.contains(check), &mut run);
            for keyed in &listed.keyed {
                platforms.insert(keyed.listed.check.clone(), keyed.key.platform);
            }
        }
        runner.run_prepared(ctx, &open, prepared, options, &mut run);
        run.outcomes
            .iter()
            .map(|(check, outcome)| CheckVerdict {
                check: check.clone(),
                outcome: *outcome,
                platform: platforms.get(check).copied().flatten(),
                output: run
                    .failures
                    .iter()
                    .find(|failure| failure.check == *check)
                    .map(failure_text)
                    .unwrap_or_default(),
            })
            .collect()
    }

    fn run_family(
        &self,
        ctx: &Context,
        tree: &GateTree,
        family: &str,
        options: &RunOptions,
    ) -> CheckRun {
        let mut run = CheckRun::default();
        let (runner, open) = match self.checks.open(ctx, &tree.tree) {
            Ok(opened) => opened,
            Err(error) => {
                run.incomplete_families
                    .push((format!("tree {}: {error}", tree.tree), PathBuf::new()));
                return run;
            }
        };
        // The family is looked up in the tree's manifests, which a landing may change.
        if !runner.families().any(|(name, _)| name == family) {
            run.incomplete_families
                .push((format!("no check family {family}"), PathBuf::new()));
            return run;
        }
        let mut prepared = runner.prepare(ctx, &open, &[family], options, &mut run);
        for listed in &mut prepared {
            listed.keep(|_| true, &mut run);
        }
        runner.run_prepared(ctx, &open, prepared, options, &mut run);
        run
    }
}

/// The lines of a regression ticket's output a failure gives: where it failed, its message,
/// and the end of the file that holds its whole output, when that can be read.
fn failure_text(failure: &Failure) -> String {
    let mut lines: Vec<String> = failure.at.iter().chain(&failure.message).cloned().collect();
    if let Some(text) = failure
        .output
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
    {
        let all: Vec<&str> = text.lines().collect();
        let start = all.len().saturating_sub(OUTPUT_LINES);
        lines.extend(all[start..].iter().map(|line| (*line).to_owned()));
    }
    lines.join("\n")
}

/// The lines of a failure's output a regression ticket keeps, as the hub keeps them.
const OUTPUT_LINES: usize = 40;
