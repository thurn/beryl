//! The machine's build trees: which are leased, to whom, and what each last built.
//!
//! Stub. The ticket Thrh4.14 (Daemon local store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use crate::Result;
use crate::local::rows;

pub fn all(conn: &Connection) -> Result<Vec<rows::Tree>> {
    todo!("Thrh4.14 (Daemon local store)")
}

pub fn upsert(conn: &Connection, tree: &rows::Tree) -> Result<()> {
    todo!("Thrh4.14 (Daemon local store)")
}
