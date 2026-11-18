//! The paths that differ between two commits.
//!
//! Stub. The ticket Thrh4.10 (Git layer: trees, object IDs, rebase and push) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

pub fn between(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<String>> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}
