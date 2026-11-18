//! Checks: their state, their record of runs and failures by area, and their costs.
//!
//! Stub. The ticket Thrh4.12 (Hub check store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::hub::{CheckState, CheckStatsReply, FlakyReport};
use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::rows;

/// Adds or updates the checks a family lists.
pub fn upsert(conn: &mut Connection, checks: &[rows::Check]) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

pub fn set_state(conn: &Connection, check: &str, state: CheckState) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

/// Counts one run (and maybe a failure) after a change to each of `areas`.
pub fn count_run(conn: &mut Connection, check: &str, areas: &[String], failed: bool) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

/// Adds a cost, keeping the last 20 and their median.
pub fn add_cost(conn: &Connection, check: &str, cost: f64, unit: &str) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

pub fn stats(conn: &Connection, family: &str) -> Result<CheckStatsReply> {
    todo!("Thrh4.12 (Hub check store)")
}

pub fn record_flaky(conn: &Connection, report: &FlakyReport, now: Timestamp) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}
