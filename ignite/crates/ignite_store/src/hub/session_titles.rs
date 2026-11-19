//! The `session_titles` table: the latest title each Claude desktop session reported, which the
//! dashboard shows in place of the session's id.

use std::collections::HashMap;

use ignite_core::time::Timestamp;
use rusqlite::{Connection, params};

use crate::Result;

/// The longest title kept, in characters. A longer one is cut, so a runaway title cannot grow
/// every page that names its session.
pub const MAX_CHARS: usize = 200;

/// Saves `title` as `session`'s title. An empty title is ignored.
pub fn record(conn: &Connection, session: &str, title: &str, now: Timestamp) -> Result<()> {
    let title: String = title.trim().chars().take(MAX_CHARS).collect();
    if session.is_empty() || title.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO session_titles (session, title, updated_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(session) DO UPDATE SET title = excluded.title, \
         updated_at = excluded.updated_at",
        params![session, title, now.to_string()],
    )?;
    Ok(())
}

/// Every saved title, by session.
pub fn all(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut statement = conn.prepare("SELECT session, title FROM session_titles")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_non_empty_title_per_session() {
        let conn = crate::hub::open_in_memory().unwrap();
        let now = Timestamp::parse("2026-11-19T06:00:00Z").unwrap();
        record(&conn, "local_a", " First ", now).unwrap();
        record(&conn, "local_a", "Second", now).unwrap();
        record(&conn, "local_a", "  ", now).unwrap();
        record(&conn, "local_b", &"x".repeat(300), now).unwrap();
        let titles = all(&conn).unwrap();
        assert_eq!(titles["local_a"], "Second");
        assert_eq!(titles["local_b"].chars().count(), MAX_CHARS);
    }
}
