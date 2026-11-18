//! Object IDs of paths in a working tree, uncommitted edits included, through a temporary index
//! so the branch and the real index are never touched.
//!
//! Stub. The ticket Thrh4.10 (Git layer: trees, object IDs, rebase and push) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::ids::Oid;
use ignite_core::traits::Result;

/// The blob ID of each file path and the tree ID of each directory path.
pub fn object_ids(tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
    todo!("Thrh4.10 (Git layer: trees, object IDs, rebase and push)")
}
