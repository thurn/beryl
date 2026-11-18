//! Tips: each new `staging` commit, its verification claims and its state, and regressions.
//!
//! Stub. The ticket Thrh4.12 (Hub check store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::hub::{RegressionReport, TipClaim, TipClaimReply, TipVerified};
use ignite_core::time::Timestamp;

use crate::Result;

pub fn seen(conn: &Connection, commit: &str, now: Timestamp) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

/// Grants the claim if no unexpired claim holds the same batch.
pub fn claim(conn: &Connection, claim: &TipClaim, now: Timestamp) -> Result<TipClaimReply> {
    todo!("Thrh4.12 (Hub check store)")
}

pub fn verified(conn: &Connection, verified: &TipVerified, now: Timestamp) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}

/// Records a regression against the ticket filed for it.
pub fn regression(conn: &Connection, report: &RegressionReport, ticket: &str) -> Result<()> {
    todo!("Thrh4.12 (Hub check store)")
}
