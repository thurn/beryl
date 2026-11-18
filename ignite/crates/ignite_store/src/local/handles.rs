//! Handles: operations that outlived a client's wait, with their replies once finished.
//!
//! Stub. The ticket Thrh4.14 (Daemon local store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use crate::Result;
use crate::local::rows;

pub fn get(conn: &Connection, id: &str) -> Result<Option<rows::Handle>> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn upsert(conn: &Connection, handle: &rows::Handle) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}
