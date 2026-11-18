//! `POST /batch`: preconditions and operations applied in one transaction that succeeds or fails
//! as a whole.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::tickets::{BatchConflict, BatchReply, BatchRequest};
use ignite_core::time::Timestamp;

use crate::Result;

/// The outcome of a batch: applied, or refused at one precondition or operation.
#[derive(Debug)]
pub enum BatchOutcome {
    Applied(BatchReply),
    Conflict(Box<BatchConflict>),
}

pub fn apply(
    conn: &mut Connection,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
) -> Result<BatchOutcome> {
    todo!("Thrh4.11 (Hub ticket store)")
}
