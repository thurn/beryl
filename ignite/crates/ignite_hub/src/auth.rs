//! Write authorization: `Authorization: Bearer <token>` names a registered machine, and `X-Actor` the
//! session or person acting.
//!
//! Stub. The ticket Thrh4.16 (Hub ticket API) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::ids::MachineId;

/// Who is writing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Writer {
    pub machine: MachineId,
    pub actor: String,
}

/// Why a write was refused (HTTP 401).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unauthorized(pub String);

/// Checks the two headers of a write.
pub fn authorize(
    conn: &Connection,
    authorization: Option<&str>,
    actor: Option<&str>,
) -> Result<Writer, Unauthorized> {
    todo!("Thrh4.16 (Hub ticket API)")
}
