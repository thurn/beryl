//! The result cache: outcomes by check, input hash and platform.
//!
//! Stub. The ticket Thrh4.12 (Hub check store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::hub::{CachedResult, NewResult, ResultKey};
use ignite_core::time::Timestamp;

use crate::Result;

/// One entry per key, in order, marking each hit as looked up at `now`.
pub fn lookup(
    conn: &Connection,
    keys: &[ResultKey],
    now: Timestamp,
) -> Result<Vec<Option<CachedResult>>> {
    todo!("Thrh4.12 (Hub check store)")
}

pub fn store(conn: &mut Connection, results: &[NewResult], now: Timestamp) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

/// Deletes entries nobody has looked up for 30 days; returns how many.
pub fn prune(conn: &Connection, now: Timestamp) -> Result<u64> {
    todo!("Thrh4.12 (Hub check store)")
}
