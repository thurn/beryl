//! Tickets: reading, listing and the single-ticket writes behind the batch route.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{Ticket, TicketDetail, TicketQuery};
use ignite_core::time::Timestamp;

use crate::Result;

/// One ticket, found by ID or by any alias, with its edges, children and readiness.
pub fn get(
    conn: &Connection,
    id: &str,
    comments: bool,
    history: bool,
    now: Timestamp,
) -> Result<Option<TicketDetail>> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// The tickets a `GET /tickets` query selects.
pub fn list(conn: &Connection, query: &TicketQuery, now: Timestamp) -> Result<Vec<Ticket>> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// The ticket an alias names, or the ID itself if it is a ticket.
pub fn resolve(conn: &Connection, id_or_alias: &str) -> Result<Option<TicketId>> {
    todo!("Thrh4.11 (Hub ticket store)")
}
