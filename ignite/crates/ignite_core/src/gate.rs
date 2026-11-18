//! Gate selection: which checks run before a landing and which are left to tip verification.
//!
//! Stub. The ticket Thrh4.4 (Check scores and gate selection) owns this file and replaces the
//! `todo!()` bodies.
#![allow(unused_variables)]

use crate::config::Gate;
use crate::ids::CheckName;

/// One check the change affects.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub check: CheckName,
    pub score: f64,
    /// Billions of instructions.
    pub cost: f64,
    /// In a family marked `always = true`.
    pub always: bool,
    /// The change edits the check itself.
    pub edited: bool,
}

/// Where each affected check runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub always: Vec<CheckName>,
    pub gate: Vec<CheckName>,
    pub tip: Vec<CheckName>,
}

pub fn select(candidates: &[Candidate], gate: &Gate) -> Selection {
    todo!("Thrh4.4 (Check scores and gate selection)")
}
