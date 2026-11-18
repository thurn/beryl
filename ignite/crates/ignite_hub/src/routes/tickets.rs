//! The ticket routes: `GET /tickets`, `GET /tickets/<id>`, `GET /graph`, `POST /batch`,
//! `POST /leases/<name>`, `GET /leases`, `GET /export` and `POST /import`.
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
