//! Build trees as git worktrees: adding one at a commit, resetting it to a clean checkout
//! that keeps its ignored build directories, listing and removing them.
//!
//! Stub. The ticket Thrh4.10 (Git layer: trees, object IDs, rebase and push) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::{Path, PathBuf};

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

/// A worktree as `git worktree list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<Commit>,
    pub branch: Option<String>,
}

/// Adds a worktree at `path` on a new or reset `branch` at `at`.
pub fn add(repo: &Path, path: &Path, branch: &str, at: &Commit) -> Result<()> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}

/// Checks out `branch` at `at` in an existing tree and removes untracked files that are not
/// ignored, keeping build directories.
pub fn reset(tree: &Path, branch: &str, at: &Commit) -> Result<()> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}

pub fn list(repo: &Path) -> Result<Vec<Worktree>> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}

pub fn remove(repo: &Path, path: &Path) -> Result<()> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}
