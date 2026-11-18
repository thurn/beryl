//! Per-machine write tokens: issued at registration, stored as hashes in `meta`.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::ids::MachineId;

use crate::Result;

/// Issues a new token for `machine`, replacing any earlier one, and returns it in clear.
pub fn issue(conn: &Connection, machine: &MachineId) -> Result<String> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// The machine a token belongs to.
pub fn verify(conn: &Connection, token: &str) -> Result<Option<MachineId>> {
    todo!("Thrh4.11 (Hub ticket store)")
}
