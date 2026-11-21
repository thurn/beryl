//! Everything Ignite does with git, by running the `git` executable.
//!
//! [`GitCli`] implements [`ignite_core::traits::Git`] for one repository by calling the
//! functions of the modules below.

pub mod changed;
mod cmd;
pub mod commit;
pub mod committed;
pub mod fastforward;
pub mod holds;
mod objects;
pub mod push;
pub mod rebase;
pub mod refs;
pub mod replay;
pub mod save;
pub mod tracked;
pub mod treehash;
pub mod worktree;

use std::path::{Path, PathBuf};

use ignite_core::ids::{Commit, Oid};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Git, Push, Rebase, Result, RevertOutcome};

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

    fn commit_paths(&self, commit: &Commit) -> Result<Vec<String>> {
        changed::in_commit(&self.repo, commit)
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

    fn known_tip(&self, branch: &str) -> Result<Commit> {
        refs::cached_tip(&self.repo, &self.remote, branch)
    }

    fn local_ref(&self, name: &str) -> Result<Option<Commit>> {
        refs::local_ref(&self.repo, name)
    }

    fn set_local_ref(&self, tree: &Path, name: &str, commit: &Commit) -> Result<()> {
        refs::set_local_ref(tree, name, commit)
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

    fn replay(&self, repo: &Path, head: &Commit, tip: &Commit) -> Result<Rebase> {
        replay::replay(repo, head, tip)
    }

    fn revert(
        &self,
        repo: &Path,
        tip: &Commit,
        commits: &[Commit],
        message: &str,
    ) -> Result<RevertOutcome> {
        replay::revert(repo, tip, commits, message)
    }

    fn subject(&self, repo: &Path, commit: &Commit) -> Result<String> {
        refs::subject(repo, commit)
    }

    fn merge_base(&self, repo: &Path, a: &Commit, b: &Commit) -> Result<Commit> {
        objects::merge_base(repo, a, b)
    }

    fn committed_at(&self, repo: &Path, commit: &Commit) -> Result<Timestamp> {
        objects::committed_at(repo, commit)
    }

    fn tree_id(&self, tree: &Path) -> Result<String> {
        Ok(treehash::write_working_tree(tree)?.as_str().to_owned())
    }
}
