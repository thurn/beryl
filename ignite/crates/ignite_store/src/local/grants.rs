//! Grants of cores and resources to running commands, so a restarted daemon knows what is held.
//!
//! Stub. The ticket Thrh4.14 (Daemon local store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use crate::Result;
use crate::local::rows;

pub fn active(conn: &Connection) -> Result<Vec<rows::Grant>> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn insert(conn: &Connection, grant: &rows::Grant) -> Result<i64> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn remove(conn: &Connection, id: i64) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}
