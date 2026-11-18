//! Every ticket, with its aliases, edges, comments and history, from the old ticket service's
//! database file, which is only read.
//!
//! Stub. The ticket Thrh4.15 (Import from the old systems) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use std::path::Path;

use crate::Result;
use crate::import::ImportReport;

pub fn import(hub: &mut Connection, old_database: &Path) -> Result<ImportReport> {
    todo!("Thrh4.15 (Import from the old systems)")
}
