//! Everything Ignite does with git, by running the `git` executable.
//!
//! [`GitCli`] implements [`ignite_core::traits::Git`] for one repository by calling the
//! functions of the modules below.

pub mod changed;
mod cmd;
pub mod committed;
pub mod fastforward;
pub mod holds;
pub mod push;
pub mod rebase;
pub mod refs;
pub mod save;
pub mod tracked;
pub mod treehash;
pub mod worktree;

use std::path::{Path, PathBuf};

use ignite_core::ids::{Commit, Oid};
use ignite_core::traits::{Git, Push, Rebase, Result};

/// One repository: the main checkout, and the remote that holds the delivery branches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCli {
    pub repo: PathBuf,
    pub remote: String,
}

impl Git for GitCli {
    fn object_ids(&self, tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
        treehash::object_ids(tree, paths)
    }

    fn changed(&self, from: &Commit, to: &Commit) -> Result<Vec<String>> {
        changed::between(&self.repo, from, to)
    }

    fn rebase_onto(&self, tree: &Path, tip: &Commit) -> Result<Rebase> {
        rebase::onto(tree, tip)
    }

    fn push(&self, tree: &Path, commit: &Commit, branch: &str) -> Result<Push> {
        push::push(tree, &self.remote, commit, branch)
    }

    fn tip(&self, branch: &str) -> Result<Commit> {
        refs::remote_tip(&self.repo, &self.remote, branch)
    }

    fn is_ancestor(&self, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        refs::is_ancestor(&self.repo, ancestor, descendant)
    }

    fn holds_changes(&self, history: &Commit, version: &Commit, path: &str) -> Result<bool> {
        holds::holds_changes(&self.repo, history, version, path)
    }

    fn replace(
        &self,
        tree: &Path,
        commit: &Commit,
        branch: &str,
        expected: &Commit,
    ) -> Result<Push> {
        push::replace(tree, &self.remote, commit, branch, expected)
    }
}
