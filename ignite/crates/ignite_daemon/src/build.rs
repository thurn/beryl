//! Build products: one build per target directory at a time, joined by requests for the same
//! product in the same tree state, and skipped when the tree already holds it.
//!
//! Stub. The ticket Thrh4.27 (Build products) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::ids::TreeId;
use ignite_core::traits::{BuildResult, Builds};

#[derive(Debug, Default)]
pub struct BuildManager;

impl Builds for BuildManager {
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        todo!("Thrh4.27 (Build products)")
    }
}
