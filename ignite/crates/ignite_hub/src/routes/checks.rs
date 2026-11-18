//! The result cache, check and record routes: `/results/lookup`, `/results/store`, `/flaky`,
//! `/checks/stats`, `/tips/claim`, `/tips/verified`, `/regressions`, `/records` and
//! `/metrics/import`.
//!
//! Stub. The ticket Thrh4.17 (Hub check and record API) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    todo!("Thrh4.17 (Hub check and record API)")
}
