//! Dependency edges.
//!
//! Stub. The ticket Thrh4.11 (Hub ticket store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::tickets::{Edge, EdgeKind};

use crate::Result;

/// Edges in both directions of one ticket.
pub fn of(conn: &Connection, id: &str) -> Result<Vec<Edge>> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// Every edge between live tickets, or between all tickets with `all`.
pub fn graph(conn: &Connection, all: bool) -> Result<Vec<Edge>> {
    todo!("Thrh4.11 (Hub ticket store)")
}

/// Adds `src` depends on `dst`, or changes the kind of an edge that exists.
pub fn add(conn: &Connection, src: &str, dst: &str, kind: EdgeKind) -> Result<()> {
    todo!("Thrh4.11 (Hub ticket store)")
}

pub fn remove(conn: &Connection, src: &str, dst: &str) -> Result<()> {
    todo!("Thrh4.11 (Hub ticket store)")
}
