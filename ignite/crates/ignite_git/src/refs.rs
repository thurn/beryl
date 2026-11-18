//! Reading and fetching refs: a remote branch's tip, a tree's HEAD, commits between two points.
//!
//! Stub. The ticket Thrh4.10 (Git layer: trees, object IDs, rebase and push) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

/// Fetches `branch` from `remote` and returns its newest commit.
pub fn remote_tip(repo: &Path, remote: &str, branch: &str) -> Result<Commit> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}

/// The commit a tree has checked out.
pub fn head(tree: &Path) -> Result<Commit> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}

/// The commits after `from` up to and including `to`, oldest first.
pub fn commits_between(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}
