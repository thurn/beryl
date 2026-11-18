//! Named, expiring leases with a queue of waiters.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::tickets::{Lease, LeaseOp, LeaseReply};
use ignite_core::time::Timestamp;

use crate::Result;

pub fn apply(
    conn: &mut Connection,
    name: &str,
    op: &LeaseOp,
    machine: &str,
    now: Timestamp,
) -> Result<LeaseReply> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// Every lease that has not expired.
pub fn list(conn: &Connection, now: Timestamp) -> Result<Vec<Lease>> {
    todo!("Thrh4.11 (Hub ticket store)")
}
