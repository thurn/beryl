//! Check scores: the chance a check fails after a change, per billion instructions it costs.
//!
//! ```text
//! score(check, change) = P(fail | areas the change touches) / cost(check)
//! P     = (failures after changes to these areas + 20 x family rate)
//!         / (runs after changes to these areas + 20)
//! ```
//!
//! Stub. The ticket Thrh4.4 (Check scores and gate selection) owns this file and replaces the
//! `todo!()` bodies.
#![allow(unused_variables)]

/// What the hub knows of one check, for the areas one change touches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckRecord {
    pub runs_in_areas: u32,
    pub failures_in_areas: u32,
    pub family_rate: f64,
    /// The median cost of its last 20 runs, in billions of instructions.
    pub median_cost: f64,
}

/// The smoothed chance that the check fails after the change.
pub fn probability(record: &CheckRecord) -> f64 {
    todo!("Thrh4.4 (Check scores and gate selection)")
}

/// The score, counting `unpaid_builds` billions for builds nothing chosen so far has paid for.
pub fn score(record: &CheckRecord, unpaid_builds: f64) -> f64 {
    todo!("Thrh4.4 (Check scores and gate selection)")
}
