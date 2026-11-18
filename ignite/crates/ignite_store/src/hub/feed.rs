//! The numbered change feed. `meta.seq` is the hub's global position: every write takes the next
//! one, and a ticket's `version` is the position of its last change. Each write also leaves one
//! change per thing it touched in the `feed` table, at the write's final position, so that a
//! client long-polling `GET /feed?since=<seq>` learns what changed without reading everything.
//!
//! The journal is bounded: only the changes of the last [`JOURNAL_SPAN`] positions are kept. A
//! client asking from before that gets a single change of kind [`RESYNC`], meaning it must read
//! the state it cares about again.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::ids::{Seq, TicketId};
use ignite_core::protocol::tickets::{FeedBatch, FeedChange};

use crate::{Result, StoreError};

/// How many positions of changes the journal keeps.
pub const JOURNAL_SPAN: u64 = 4096;

/// The kind of the one change sent to a client whose position is older than the journal.
pub const RESYNC: &str = "resync";

/// The newest position.
pub fn current(conn: &Connection) -> Result<Seq> {
    Ok(Seq(meta_number(conn, "seq")?))
}

/// Takes the next position for a write in progress. Call it inside the write's transaction.
pub fn next_seq(conn: &Connection) -> Result<Seq> {
    let next = Seq(current(conn)?.0 + 1);
    set_current(conn, next)?;
    Ok(next)
}

/// Moves the global position to `seq`, for a write that numbered several changes itself.
pub fn set_current(conn: &Connection, seq: Seq) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('seq', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [seq.0.to_string()],
    )?;
    Ok(())
}

/// Records a change at its `seq`, once, and forgets changes that have left the journal's span.
pub fn record(conn: &Connection, change: &FeedChange) -> Result<()> {
    let ticket = change.ticket.as_ref().map(TicketId::as_str);
    let seq = change.seq.0 as i64;
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM feed WHERE seq = ?1 AND kind = ?2 AND ticket IS ?3 AND name IS ?4",
            params![seq, change.kind, ticket, change.name],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        conn.execute(
            "INSERT INTO feed (seq, kind, ticket, name) VALUES (?1, ?2, ?3, ?4)",
            params![seq, change.kind, ticket, change.name],
        )?;
    }
    let floor = change.seq.0.saturating_sub(JOURNAL_SPAN);
    if floor > meta_number(conn, "feed_floor")? {
        conn.execute("DELETE FROM feed WHERE seq <= ?1", [floor as i64])?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('feed_floor', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [floor.to_string()],
        )?;
    }
    Ok(())
}

/// The changes after `since`, oldest first. A position's changes are never split: the batch
/// ends before the position that would take it past `limit` changes, but always holds the
/// first position in full, so a client that continues from the last change's `seq` misses
/// nothing. A client older than the journal gets one [`RESYNC`] change at the current position.
pub fn since(conn: &Connection, since: Seq, limit: u32) -> Result<FeedBatch> {
    let seq = current(conn)?;
    if since >= seq {
        return Ok(FeedBatch {
            seq,
            changes: Vec::new(),
        });
    }
    if since.0 < meta_number(conn, "feed_floor")? {
        return Ok(FeedBatch {
            seq,
            changes: vec![FeedChange {
                seq,
                kind: RESYNC.to_owned(),
                ticket: None,
                name: None,
            }],
        });
    }
    let mut statement =
        conn.prepare("SELECT seq, kind, ticket, name FROM feed WHERE seq > ?1 ORDER BY seq, id")?;
    let rows = statement.query_map([since.0 as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut changes: Vec<FeedChange> = Vec::new();
    for row in rows {
        let (position, kind, ticket, name) = row?;
        let position = Seq(position as u64);
        let starts_position = changes.last().is_none_or(|last| last.seq != position);
        if starts_position && changes.len() >= limit as usize {
            break;
        }
        let ticket = ticket
            .map(|id| {
                id.parse()
                    .map_err(|e| StoreError::Invalid(format!("feed: {e}")))
            })
            .transpose()?;
        changes.push(FeedChange {
            seq: position,
            kind,
            ticket,
            name,
        });
    }
    Ok(FeedBatch { seq, changes })
}

fn meta_number(conn: &Connection, key: &str) -> Result<u64> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    match value {
        None => Ok(0),
        Some(text) => text
            .parse()
            .map_err(|_| StoreError::Invalid(format!("meta.{key} is {text:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;

    fn change(seq: u64, ticket: &str) -> FeedChange {
        FeedChange {
            seq: Seq(seq),
            kind: "ticket".to_owned(),
            ticket: Some(ticket.parse().unwrap()),
            name: None,
        }
    }

    fn write(conn: &Connection, tickets: &[&str]) -> Seq {
        let seq = next_seq(conn).unwrap();
        for ticket in tickets {
            record(conn, &change(seq.0, ticket)).unwrap();
        }
        seq
    }

    #[test]
    fn positions_count_up_from_zero() {
        let conn = open_in_memory().unwrap();
        assert_eq!(current(&conn).unwrap(), Seq(0));
        assert_eq!(next_seq(&conn).unwrap(), Seq(1));
        assert_eq!(next_seq(&conn).unwrap(), Seq(2));
        assert_eq!(current(&conn).unwrap(), Seq(2));
    }

    #[test]
    fn a_client_reads_what_changed_after_its_position_once_each() {
        let conn = open_in_memory().unwrap();
        let first = write(&conn, &["a", "b", "a"]);
        let second = write(&conn, &["c"]);
        let all = since(&conn, Seq(0), 100).unwrap();
        assert_eq!(all.seq, second);
        assert_eq!(
            all.changes,
            [change(1, "a"), change(1, "b"), change(2, "c")]
        );
        assert_eq!(since(&conn, first, 100).unwrap().changes, [change(2, "c")]);
        assert!(since(&conn, second, 100).unwrap().changes.is_empty());
    }

    #[test]
    fn a_limited_batch_never_splits_a_position() {
        let conn = open_in_memory().unwrap();
        write(&conn, &["a", "b", "c"]);
        write(&conn, &["d"]);
        let batch = since(&conn, Seq(0), 2).unwrap();
        assert_eq!(batch.changes.len(), 3, "the first position comes whole");
        let rest = since(&conn, batch.changes[2].seq, 2).unwrap();
        assert_eq!(rest.changes, [change(2, "d")]);
    }

    #[test]
    fn a_client_older_than_the_journal_is_told_to_resync() {
        let conn = open_in_memory().unwrap();
        set_current(&conn, Seq(JOURNAL_SPAN + 9)).unwrap();
        write(&conn, &["a"]);
        let batch = since(&conn, Seq(5), 100).unwrap();
        assert_eq!(batch.changes.len(), 1);
        assert_eq!(batch.changes[0].kind, RESYNC);
        assert_eq!(batch.changes[0].seq, batch.seq);
        let recent = since(&conn, Seq(JOURNAL_SPAN + 9), 100).unwrap();
        assert_eq!(recent.changes, [change(JOURNAL_SPAN + 10, "a")]);
    }
}
