//! The outbox: records and results queue in the local database and are sent to the hub in order
//! whenever it is reachable.
//!
//! Stub. The ticket Thrh4.23 (Daemon core: requests, handles and the hub client) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::hub::{NewResult, Record};
use ignite_core::traits::{Hub, Result};

pub fn push_record(conn: &Connection, record: &Record) -> Result<()> {
    todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
}

pub fn push_result(conn: &Connection, result: &NewResult) -> Result<()> {
    todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
}

/// Sends what is queued; returns how many entries were sent.
pub fn flush(conn: &Connection, hub: &dyn Hub) -> Result<u32> {
    todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
}
