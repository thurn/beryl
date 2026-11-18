//! The old run log and verdicts on check failures, as each check's runs, costs and failures, so
//! the gate has scores on the first day.
//!
//! Stub. The ticket Thrh4.15 (Import from the old systems) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use std::path::Path;

use crate::Result;
use crate::import::ImportReport;

pub fn import(hub: &mut Connection, run_log: &Path) -> Result<ImportReport> {
    todo!("Thrh4.15 (Import from the old systems)")
}
