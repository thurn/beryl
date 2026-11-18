//! The daemon's HTTP client for the hub, on `ureq`.
//!
//! Stub. The ticket Thrh4.23 (Daemon core: requests, handles and the hub client) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::ids::Seq;
use ignite_core::protocol::hub::{
    CachedResult, Claim, Landed, MachineFree, MetricsImport, NewResult, Record, Reservation,
    ResultKey, TicketBrief,
};
use ignite_core::protocol::tickets::{FeedBatch, LeaseOp, LeaseReply};
use ignite_core::traits::{Hub, Result};

/// The hub at `base` (`<url>/<project>/api`), writing as `actor` with the machine's token.
#[derive(Clone, Debug)]
pub struct HubClient {
    pub base: String,
    pub token: String,
    pub actor: String,
}

impl Hub for HubClient {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn store(&self, results: &[NewResult]) -> Result<()> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn claim(&self, claim: &Claim) -> Result<TicketBrief> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn landed(&self, landed: &Landed) -> Result<()> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn feed(&self, since: Seq, wait_s: u32) -> Result<FeedBatch> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn records(&self, batch: &[Record]) -> Result<()> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    fn metrics(&self, import: &MetricsImport) -> Result<()> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }
}
