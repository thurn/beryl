//! The fleet and hand-off routes: `/machines/register`, `/machines/heartbeat`, `/next`, `/claims`,
//! `/landed` and `/release`.
//!
//! Stub. The ticket Thrh4.18 (Hub dispatch API) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    todo!("Thrh4.18 (Hub dispatch API)")
}
