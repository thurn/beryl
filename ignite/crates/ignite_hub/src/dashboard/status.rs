//! The dashboard's `status` page.
//!
//! Stub. The ticket Thrh4.19 (Dashboard: the current ticket views in Rust) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

/// The page's HTML, or `None` if `path` is not this page's.
pub fn render(conn: &Connection, path: &str, now: Timestamp) -> Option<String> {
    todo!("Thrh4.19 (Dashboard: the current ticket views in Rust)")
}
