//! Build trees: a fixed set of ticket and landing trees per machine, leased by ticket, kept
//! built near the tip and held under the machine's disk limit.
//!
//! Stub. The ticket Thrh4.26 (Build trees) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::PathBuf;

use ignite_core::ids::{Commit, TicketId, TreeId};

/// A leased tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeLease {
    pub tree: TreeId,
    pub path: PathBuf,
    pub branch: String,
    pub at: Commit,
}

#[derive(Debug, Default)]
pub struct Trees;

impl Trees {
    /// Leases a free ticket tree at `tip`: the one built nearest the tip for a ticket expected
    /// to compile Rust, otherwise the one with the oldest build.
    pub fn lease(&self, ticket: &TicketId, tip: &Commit, compiles: bool) -> Option<TreeLease> {
        todo!("Thrh4.26 (Build trees)")
    }

    /// Resets the tree to a clean checkout, keeping its build directories.
    pub fn release(&self, tree: &TreeId) {
        todo!("Thrh4.26 (Build trees)")
    }
}
