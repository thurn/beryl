//! Fleet metrics: each daemon runs the efficiency ingest for its own machine and posts the new
//! rows to the hub.
//!
//! Stub. The ticket Thrh4.35 (Fleet metrics on the hub) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use crate::server::Context;

/// Runs the ingest and posts its new rows; returns how many were posted.
pub fn post_new_rows(ctx: &Context) -> ignite_core::traits::Result<u32> {
    todo!("Thrh4.35 (Fleet metrics on the hub)")
}
