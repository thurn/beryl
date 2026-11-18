//! Admission: whether a ticket's expected use of cores fits beside the tickets in progress.
//!
//! Stub. The ticket Thrh4.5 (Dispatch order, shares and admission) owns this file and replaces
//! the `todo!()` bodies.
#![allow(unused_variables)]

/// A ticket's expected cores from a planner's figure, or from the records of tickets that named
/// the same files.
pub fn expected_cores(planner: Option<f64>, similar: &[f64]) -> f64 {
    todo!("Thrh4.5 (Dispatch order, shares and admission)")
}

/// Whether a ticket expecting `expected` cores fits under `admit_cores` beside `in_progress`.
pub fn admits(admit_cores: f64, in_progress: &[f64], expected: f64) -> bool {
    todo!("Thrh4.5 (Dispatch order, shares and admission)")
}
