//! Shares: dividing idle workers between epics, and the share of hand-offs owed to improvement
//! tickets.
//!
//! Stub. The ticket Thrh4.5 (Dispatch order, shares and admission) owns this file and replaces
//! the `todo!()` bodies.
#![allow(unused_variables)]

use crate::config::Improvement;
use crate::ids::TicketId;

/// One epic's claim on workers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicDemand {
    pub epic: TicketId,
    /// The epic's `--workers` target.
    pub workers: Option<u32>,
    /// The epic's `--max-workers` cap.
    pub max_workers: Option<u32>,
    pub ready: u32,
    /// Workers holding its tickets now.
    pub held: u32,
}

/// How many of `idle` workers each epic should get, in the order given.
pub fn divide(idle: u32, epics: &[EpicDemand]) -> Vec<(TicketId, u32)> {
    todo!("Thrh4.5 (Dispatch order, shares and admission)")
}

/// The improvement share for a backlog whose saving is `backlog_fraction` of last week's
/// total, in the largest of the three units.
pub fn improvement_share(backlog_fraction: f64, config: &Improvement) -> f64 {
    todo!("Thrh4.5 (Dispatch order, shares and admission)")
}
