//! Pushing a commit to a remote branch, refused unless it fast-forwards.
//!
//! Stub. The ticket Thrh4.10 (Git layer: trees, object IDs, rebase and push) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Push, Result};

pub fn push(tree: &Path, remote: &str, commit: &Commit, branch: &str) -> Result<Push> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}
