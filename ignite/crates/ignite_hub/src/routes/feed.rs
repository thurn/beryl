//! `GET /feed?since=<seq>&wait=<seconds>`: returns as soon as anything after `since` exists, or
//! empty when the wait ends.
//!
//! Stub. The ticket Thrh4.16 (Hub ticket API) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    todo!("Thrh4.16 (Hub ticket API)")
}
