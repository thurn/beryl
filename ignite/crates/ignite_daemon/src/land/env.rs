//! What a landing needs besides the traits in [`Context`]: the machine's build trees
//! ([`LandTrees`]) and the gate ([`LandGate`]), with their real implementations over
//! [`Trees`] and [`Checks`]. Tests replace both with fakes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::ids::{Commit, SessionId, TicketId, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::{push, refs, save};

use crate::checks::{CheckRun, Checks, GateRun, RunOptions};
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
    /// The files in `path` that are neither committed nor ignored.
    fn uncommitted(&self, path: &Path) -> Result<Vec<String>>;
    /// The commits after `from` up to `to`, oldest first.
    fn commits(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>>;
    /// Whether `ancestor` is in the history of `descendant`, as `path`'s repository sees it.
    fn is_ancestor(&self, path: &Path, ancestor: &Commit, descendant: &Commit) -> Result<bool>;
    /// Leases a free landing tree checked out at `at`; `None` when every one is busy.
    fn lease_landing(&self, holder: &str, at: &Commit) -> Result<Option<GateTree>>;
    /// Frees a landing tree; `built_at` is the commit its builds now match.
    fn release_landing(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()>;
    /// Frees a ticket tree after its landing, parked at `park_at`. `caller` is the client's
    /// process, which is spared with its ancestors when the tree's processes are ended.
    fn release_ticket(&self, tree: &TreeId, park_at: &Commit, caller: Option<u32>) -> Result<()>;
    /// Deletes the remote branch that held the ticket's work saved at a release.
    fn delete_saved(&self, ticket: &TicketId) -> Result<()>;
}

/// The landing gate.
pub trait LandGate: Send + Sync {
    /// Runs the always set and the gate in `tree` as it is.
    fn run(&self, ctx: &Context, tree: &GateTree, options: &RunOptions) -> GateRun;
    /// Runs every check of one family in `tree` as it is, each from the result cache where it
    /// can be. A family the manifests do not declare is incomplete.
    fn run_family(
        &self,
        ctx: &Context,
        tree: &GateTree,
        family: &str,
        options: &RunOptions,
    ) -> CheckRun;
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

    fn uncommitted(&self, path: &Path) -> Result<Vec<String>> {
        save::uncommitted(path)
    }

    fn commits(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
        refs::commits_between(path, from, to)
    }

    fn is_ancestor(&self, path: &Path, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        refs::is_ancestor(path, ancestor, descendant)
    }

    fn lease_landing(&self, holder: &str, at: &Commit) -> Result<Option<GateTree>> {
        Ok(self.trees.lease_landing(holder, at)?.map(|lease| GateTree {
            tree: lease.tree,
            path: lease.path,
        }))
    }

    fn release_landing(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        self.trees.release_landing(tree, built_at)
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
    fn run(&self, ctx: &Context, tree: &GateTree, options: &RunOptions) -> GateRun {
        self.checks.gate(ctx, &tree.tree, options)
    }

    fn run_family(
        &self,
        ctx: &Context,
        tree: &GateTree,
        family: &str,
        options: &RunOptions,
    ) -> CheckRun {
        let mut run = CheckRun::default();
        if !self.checks.families().any(|(name, _)| name == family) {
            run.incomplete_families
                .push((format!("no check family {family}"), PathBuf::new()));
            return run;
        }
        let open = match self.checks.open(&tree.tree) {
            Ok(open) => open,
            Err(error) => {
                run.incomplete_families
                    .push((format!("tree {}: {error}", tree.tree), PathBuf::new()));
                return run;
            }
        };
        let mut prepared = self
            .checks
            .prepare(ctx, &open, &[family], options, &mut run);
        for listed in &mut prepared {
            listed.keep(|_| true, &mut run);
        }
        self.checks
            .run_prepared(ctx, &open, prepared, options, &mut run);
        run
    }
}
