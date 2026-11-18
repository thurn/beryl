//! Findings: what each improvement rule found, the ticket filed for it and when it closed.
//!
//! Stub. The ticket Thrh4.13 (Hub records and fleet store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::rows;

pub fn upsert(conn: &Connection, finding: &rows::Finding) -> Result<()> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

/// Whether the rule may file for the subject: no ticket for it closed within 30 days.
pub fn may_file(conn: &Connection, rule: &str, subject: &str, now: Timestamp) -> Result<bool> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

pub fn open(conn: &Connection) -> Result<Vec<rows::Finding>> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}
