//! The fleet: machines, their workers, reservations and hand-offs.
//!
//! Stub. The ticket Thrh4.13 (Hub records and fleet store) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use rusqlite::Connection;

use ignite_core::protocol::hub::{Heartbeat, MachineRegister, Reservation};
use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::rows;

pub fn register(conn: &Connection, machine: &MachineRegister, now: Timestamp) -> Result<()> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

pub fn heartbeat(conn: &mut Connection, heartbeat: &Heartbeat, now: Timestamp) -> Result<()> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

pub fn machines(conn: &Connection) -> Result<Vec<rows::Machine>> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

pub fn reserve(conn: &Connection, reservation: &Reservation) -> Result<()> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

/// Deletes reservations that expired; returns their tickets.
pub fn lapse(conn: &Connection, now: Timestamp) -> Result<Vec<String>> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

pub fn record_handoff(
    conn: &Connection,
    machine: &str,
    ticket: &str,
    kind: &str,
    now: Timestamp,
) -> Result<()> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}

/// The last `n` hand-offs to a machine, newest first.
pub fn recent_handoffs(conn: &Connection, machine: &str, n: u32) -> Result<Vec<rows::Handoff>> {
    todo!("Thrh4.13 (Hub records and fleet store)")
}
