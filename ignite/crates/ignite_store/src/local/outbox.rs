//! The outbox: records and results not yet sent to the hub, sent in order when it is reachable.
//!
//! Stub. The ticket Thrh4.14 (Daemon local store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use crate::Result;
use crate::local::rows;

pub fn push(conn: &Connection, kind: &str, body: &str, at: &str) -> Result<i64> {
    todo!("Thrh4.14 (Daemon local store)")
}

/// The oldest `limit` entries.
pub fn peek(conn: &Connection, limit: u32) -> Result<Vec<rows::OutboxEntry>> {
    todo!("Thrh4.14 (Daemon local store)")
}

/// Deletes entries up to and including `seq`.
pub fn ack(conn: &Connection, seq: i64) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}
