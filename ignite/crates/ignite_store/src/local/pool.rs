//! The pool: the machine's worker and dispatcher sessions.
//!
//! Stub. The ticket Thrh4.14 (Daemon local store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use crate::Result;
use crate::local::rows;

pub fn all(conn: &Connection) -> Result<Vec<rows::PoolMember>> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn upsert(conn: &Connection, member: &rows::PoolMember) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn remove(conn: &Connection, session: &str) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}
