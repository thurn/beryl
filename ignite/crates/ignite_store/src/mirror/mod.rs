//! A daemon's ticket mirror: an append-only copy of every version of every ticket the hub has
//! served this machine, in a database of its own beside the daemon's (`mirror.db`). The hub's
//! database is the only live copy of the tickets, and its own backups sit on the same disk, so
//! each machine keeps this copy to restore from (`ignite hub restore --from-mirror`).
//!
//! The daemon decides what to store (see the daemon's `mirror` module); this module only
//! appends versions, reads them back as of a moment, and keeps the follower's state.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::migrate::{self, Migration};

crate::tables! {
    /// One version of one ticket, as received.
    Version = "versions" {
        id: i64,
        ticket: String,
        received_at: String,
        seq: i64,
        whole_json: String,
    }

    /// One value of the follower's state.
    State = "state" {
        key: String,
        value: String,
    }
}

/// The mirror's schema, oldest step first. A later change appends a step; it never edits one.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("schema.sql"),
    code: None,
}];

pub fn open(path: &Path) -> Result<Connection> {
    migrate::open(path, MIGRATIONS)
}

pub fn open_in_memory() -> Result<Connection> {
    migrate::open_in_memory(MIGRATIONS)
}

/// The newest stored version of `ticket`.
pub fn latest(conn: &Connection, ticket: &str) -> Result<Option<Version>> {
    Ok(conn
        .query_row(
            "SELECT id, ticket, received_at, seq, whole_json FROM versions \
             WHERE ticket = ?1 ORDER BY id DESC LIMIT 1",
            [ticket],
            version_row,
        )
        .optional()?)
}

/// Appends a version of `ticket`.
pub fn append(
    conn: &Connection,
    ticket: &str,
    received_at: &str,
    seq: u64,
    whole_json: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO versions (ticket, received_at, seq, whole_json) VALUES (?1, ?2, ?3, ?4)",
        params![
            ticket,
            received_at,
            i64::try_from(seq).unwrap_or(i64::MAX),
            whole_json
        ],
    )?;
    Ok(())
}

/// The newest version of each ticket received at or before `at`, or the newest of all without
/// `at`, ordered by ticket.
pub fn as_of(conn: &Connection, at: Option<&str>) -> Result<Vec<Version>> {
    let mut statement = conn.prepare(
        "SELECT id, ticket, received_at, seq, whole_json FROM versions WHERE id IN (\
             SELECT MAX(id) FROM versions WHERE ?1 IS NULL OR received_at <= ?1 GROUP BY ticket\
         ) ORDER BY ticket",
    )?;
    let rows = statement.query_map([at], version_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// How many versions the mirror holds, and of how many tickets.
pub fn counts(conn: &Connection) -> Result<(u64, u64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COUNT(DISTINCT ticket) FROM versions",
        [],
        |row| {
            Ok((
                u64::try_from(row.get::<_, i64>(0)?).unwrap_or_default(),
                u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
            ))
        },
    )?)
}

/// One value of the follower's state.
pub fn state(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM state WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

/// Sets one value of the follower's state, or removes it for `None`.
pub fn set_state(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => conn.execute(
            "INSERT INTO state (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?,
        None => conn.execute("DELETE FROM state WHERE key = ?1", [key])?,
    };
    Ok(())
}

fn version_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Version> {
    Ok(Version {
        id: row.get(0)?,
        ticket: row.get(1)?,
        received_at: row.get(2)?,
        seq: row.get(3)?,
        whole_json: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_structs_match_the_schema() {
        let conn = open_in_memory().unwrap();
        assert_eq!(migrate::version(&conn).unwrap(), MIGRATIONS.len() as u32);
        crate::test_support::assert_rows_match_schema(&conn, TABLES);
        migrate::assert_additive_unless_breaking(MIGRATIONS);
    }

    #[test]
    fn versions_can_be_added_but_never_changed_or_removed() {
        let conn = open_in_memory().unwrap();
        append(&conn, "T.1", "2026-01-01T00:00:00.000Z", 1, "{}").unwrap();
        for sql in [
            "UPDATE versions SET whole_json = 'x'",
            "DELETE FROM versions",
        ] {
            let error = conn.execute(sql, []).unwrap_err().to_string();
            assert!(error.contains("append-only"), "{sql}: {error}");
        }
        assert_eq!(counts(&conn).unwrap(), (1, 1));
    }

    #[test]
    fn as_of_gives_each_tickets_newest_version_by_then() {
        let conn = open_in_memory().unwrap();
        append(&conn, "T.1", "2026-01-01T00:00:00.000Z", 1, "a1").unwrap();
        append(&conn, "T.2", "2026-01-01T00:00:00.000Z", 1, "b1").unwrap();
        append(&conn, "T.1", "2026-01-02T00:00:00.000Z", 2, "a2").unwrap();
        append(&conn, "T.3", "2026-01-03T00:00:00.000Z", 3, "c1").unwrap();
        let json = |versions: Vec<Version>| -> Vec<String> {
            versions.into_iter().map(|v| v.whole_json).collect()
        };
        assert_eq!(
            json(as_of(&conn, Some("2026-01-01T12:00:00.000Z")).unwrap()),
            ["a1", "b1"]
        );
        assert_eq!(json(as_of(&conn, None).unwrap()), ["a2", "b1", "c1"]);
        assert_eq!(latest(&conn, "T.1").unwrap().unwrap().whole_json, "a2");
        assert_eq!(latest(&conn, "T.9").unwrap(), None);
    }
}
