//! Dispatch order: which ready ticket the hub offers a machine that has a worker free.
//!
//! Stub. The ticket Thrh4.5 (Dispatch order, shares and admission) owns this file and replaces
//! the `todo!()` bodies.
#![allow(unused_variables)]

use crate::ids::TicketId;
use crate::protocol::hub::MachineFree;
use crate::time::Timestamp;

/// What kind of work a ready ticket is, which sets its place in the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketKind {
    Regression,
    Improvement,
    Ordinary,
}

/// A ready ticket as dispatch sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadyTicket {
    pub id: TicketId,
    pub kind: TicketKind,
    pub epic: Option<TicketId>,
    pub priority: u8,
    /// The longest chain of tickets waiting behind it.
    pub chain: u32,
    pub created_at: Timestamp,
    pub labels: Vec<String>,
    /// Expected cores while in progress.
    pub expected_cores: f64,
    /// The check families its named files could bring into its gate.
    pub families: Vec<String>,
}

/// The first ticket the machine can take, or `None`.
pub fn next_for(
    free: &MachineFree,
    ready: &[ReadyTicket],
    epic_order: &[TicketId],
    improvements_due: bool,
) -> Option<TicketId> {
    todo!("Thrh4.5 (Dispatch order, shares and admission)")
}
