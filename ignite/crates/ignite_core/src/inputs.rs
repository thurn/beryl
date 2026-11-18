//! Input resolution: from a family's `inputs` patterns to the concrete paths a check reads.
//!
//! Stub. The ticket Thrh4.3 (Input resolution and hashing) owns this file and replaces the
//! `todo!()` bodies.
#![allow(unused_variables)]

use crate::manifest::InputPattern;

/// One workspace package, as `cargo metadata` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    /// The package's directory, relative to the checkout's root.
    pub dir: String,
    /// Workspace packages it depends on, development dependencies included.
    pub deps: Vec<String>,
}

/// The workspace packages of every Cargo workspace a family reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrateGraph {
    pub packages: Vec<Package>,
}

/// A pattern that names nothing that exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputError {
    pub pattern: String,
    pub message: String,
}

/// The sorted, deduplicated paths (files and directories) that `patterns` name. `test_package`
/// is the package holding the check, which `crate-closure` starts from.
pub fn resolve(
    patterns: &[InputPattern],
    test_package: Option<&str>,
    graph: &CrateGraph,
) -> Result<Vec<String>, InputError> {
    todo!("Thrh4.3 (Input resolution and hashing)")
}

/// The package and every workspace package it depends on, transitively.
pub fn closure(package: &str, graph: &CrateGraph) -> Result<Vec<String>, InputError> {
    todo!("Thrh4.3 (Input resolution and hashing)")
}
