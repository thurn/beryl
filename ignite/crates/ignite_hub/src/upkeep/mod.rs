//! Hub upkeep, run periodically: lapsing reservations, filing regression and flaky-check
//! tickets, filing tickets for checks that stay incomplete, and freeing work held by stalled
//! sessions.
//!
//! Stub. The ticket Thrh4.22 (Hub upkeep: regressions, flaky checks and stuck work) owns this folder
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

/// What one pass did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpkeepReport {
    pub lapsed_reservations: u32,
    pub tickets_filed: Vec<String>,
}

pub fn run_once(conn: &mut Connection, now: Timestamp) -> rusqlite::Result<UpkeepReport> {
    todo!("Thrh4.22 (Hub upkeep: regressions, flaky checks and stuck work)")
}
