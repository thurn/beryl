//! Ticket readiness. A ticket is ready when it is open, not deferred, has no open blocker of its
//! own or of an ancestor, and has no open child.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file and replaces the `todo!()`
//! bodies.
#![allow(unused_variables)]

use crate::ids::TicketId;
use crate::protocol::tickets::{Blocker, Edge, Ticket};
use crate::time::Timestamp;

/// What holds each live ticket back; an empty list means ready.
pub fn blockers(
    tickets: &[Ticket],
    edges: &[Edge],
    now: Timestamp,
) -> Vec<(TicketId, Vec<Blocker>)> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// The ready tickets, in the order `blockers` received them.
pub fn ready(tickets: &[Ticket], edges: &[Edge], now: Timestamp) -> Vec<TicketId> {
    todo!("Thrh4.11 (Hub ticket store)")
}
