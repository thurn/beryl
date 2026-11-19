//! Meta-tickets and their copies (see `ignite_core::schedule`): which meta-tickets are active,
//! and what each one's newest copy says about when the next is due.

use rusqlite::{Connection, params};

use ignite_core::protocol::tickets::Ticket;
use ignite_core::schedule::{self, LastCopy, NextCopy};
use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::tickets;

/// The meta-tickets whose schedules run: open, with an interval.
pub fn active(conn: &Connection) -> Result<Vec<Ticket>> {
    tickets::select(
        conn,
        "WHERE type = 'meta' AND status = 'open' AND every_hours > 0 ORDER BY id",
        &[],
    )
}

/// What the copies of the meta-ticket `id` say about its schedule: live while any copy is not
/// closed, otherwise the time the newest copy closed.
pub fn last_copy(conn: &Connection, id: &str) -> Result<LastCopy> {
    let (live, newest): (i64, Option<String>) = conn.query_row(
        "SELECT count(*) FILTER (WHERE status != 'closed'),
                max(coalesce(closed_at, created_at))
         FROM tickets WHERE parent = ?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if live > 0 {
        return Ok(LastCopy::Live);
    }
    Ok(match newest.as_deref().map(Timestamp::parse) {
        Some(Ok(at)) => LastCopy::ClosedAt(at),
        _ => LastCopy::None,
    })
}

/// When the meta-ticket `meta` next files a copy, or `None` when its schedule is not running:
/// it is not open, or it has no interval.
pub fn next_copy(conn: &Connection, meta: &Ticket, now: Timestamp) -> Result<Option<NextCopy>> {
    let Some(every) = meta.every_hours.filter(|&n| n > 0) else {
        return Ok(None);
    };
    if meta.status != ignite_core::protocol::tickets::Status::Open {
        return Ok(None);
    }
    let last = last_copy(conn, meta.id.as_str())?;
    Ok(Some(schedule::next_copy(meta.created_at, every, last, now)))
}
