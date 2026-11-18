//! Exploration shares: how idle time is split between exploration families by their yield.
//!
//! ```text
//! yield(family) = (bugs found in 30 days + 1) / instructions spent in those days
//! share(family) = yield(family) / sum of every family's yield
//! ```
//!
//! Stub. The ticket Thrh4.6 (Verdicts and exploration shares) owns this file and replaces the
//! `todo!()` bodies.
#![allow(unused_variables)]

/// One exploration family's last 30 days.
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyYield {
    pub family: String,
    pub bugs: u32,
    /// Billions of instructions.
    pub instructions: f64,
}

/// Each family's share of idle time, summing to 1.
pub fn shares(families: &[FamilyYield]) -> Vec<(String, f64)> {
    todo!("Thrh4.6 (Verdicts and exploration shares)")
}
