//! Rebasing a tree's branch onto the tip, aborting cleanly on a conflict.
//!
//! Stub. The ticket Thrh4.10 (Git layer: trees, object IDs, rebase and push) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Rebase, Result};

pub fn onto(tree: &Path, tip: &Commit) -> Result<Rebase> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}
