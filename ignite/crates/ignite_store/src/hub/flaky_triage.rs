//! The fleet shares one ordinary flaky-check investigation slot.

use rusqlite::{Connection, params};

use ignite_core::protocol::tickets::Ticket;
use ignite_core::time::Timestamp;

use crate::Result;

pub const CATEGORY: &str = "flaky-triage";

pub fn is_batch(ticket: &Ticket) -> bool {
    ticket
        .metadata
        .get("ignite_fault")
        .and_then(|fault| fault["category"].as_str())
        == Some(CATEGORY)
}

/// A reservation occupies the slot until it expires or is replaced by a claim. Check this in
/// the same immediate transaction that reserves or claims a ticket.
pub fn slot_taken(conn: &Connection, now: Timestamp) -> Result<bool> {
    Ok(conn.query_row(
        "WITH busy(id) AS (
             SELECT id FROM tickets WHERE status = 'in_progress' AND status != 'closed'
             UNION ALL
             SELECT ticket FROM reservations WHERE expires_at > ?2
         )
         SELECT EXISTS (
             SELECT 1 FROM busy JOIN tickets t ON t.id = busy.id
             WHERE json_extract(t.metadata, '$.ignite_fault.category') = ?1
         )",
        params![CATEGORY, now.to_string()],
        |row| row.get(0),
    )?)
}
