//! The numbered change feed: every write takes the next `seq` from `meta` and leaves a change that
//! long-polling clients read.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::ids::Seq;
use ignite_core::protocol::tickets::{FeedBatch, FeedChange};

use crate::Result;

/// The newest position.
pub fn current(conn: &Connection) -> Result<Seq> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// Takes the next position for a write in progress.
pub fn next_seq(conn: &Connection) -> Result<Seq> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// Records a change at `seq`.
pub fn record(conn: &Connection, change: &FeedChange) -> Result<()> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// The changes after `since`, at most `limit`.
pub fn since(conn: &Connection, since: Seq, limit: u32) -> Result<FeedBatch> {
    todo!("Thrh4.11 (Hub ticket store)")
}
