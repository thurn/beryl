//! Records: one row per command from every machine, with per-crate compile costs.
//!
//! Stub. The ticket Thrh4.13 (Hub records and fleet store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::ids::RecordId;
use ignite_core::protocol::hub::Record;

use crate::Result;

/// Inserts the records and their compile units; returns their IDs in order.
pub fn insert(conn: &mut Connection, records: &[Record]) -> Result<Vec<RecordId>> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

/// The records of one ticket.
pub fn of_ticket(conn: &Connection, ticket: &str) -> Result<Vec<Record>> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}
