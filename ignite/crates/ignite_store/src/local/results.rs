//! The machine's copy of the result cache, used first and while the hub is unreachable.
//!
//! Stub. The ticket Thrh4.14 (Daemon local store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use crate::Result;
use crate::local::rows;

pub fn get(
    conn: &Connection,
    check: &str,
    hash: &str,
    platform: &str,
) -> Result<Option<rows::CachedResult>> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn put(conn: &Connection, result: &rows::CachedResult) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}
