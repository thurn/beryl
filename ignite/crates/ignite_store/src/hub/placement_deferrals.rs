//! The first time a ready ticket waited for a better machine.

use rusqlite::{Connection, OptionalExtension};

use ignite_core::ids::TicketId;
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

pub fn first(conn: &Connection, ticket: &TicketId) -> Result<Option<Timestamp>> {
    let at: Option<String> = conn
        .query_row(
            "SELECT first_at FROM placement_deferrals WHERE ticket = ?1",
            [ticket.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    at.map(|at| Timestamp::parse(&at).map_err(|error| StoreError::Invalid(error.to_string())))
        .transpose()
}

pub fn record(conn: &Connection, ticket: &TicketId, now: Timestamp) -> Result<()> {
    conn.execute(
        "INSERT INTO placement_deferrals (ticket, first_at) VALUES (?1, ?2)
         ON CONFLICT(ticket) DO NOTHING",
        (ticket.as_str(), now.to_string()),
    )?;
    Ok(())
}

pub fn clear(conn: &Connection, ticket: &TicketId) -> Result<()> {
    conn.execute(
        "DELETE FROM placement_deferrals WHERE ticket = ?1",
        [ticket.as_str()],
    )?;
    Ok(())
}

/// Removes waits left by older builds after a ticket left the open queue.
pub fn prune(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM placement_deferrals
         WHERE NOT EXISTS (
             SELECT 1 FROM tickets
             WHERE id = placement_deferrals.ticket AND status = 'open' AND assignee IS NULL
         )",
        [],
    )?;
    Ok(())
}
