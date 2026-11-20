//! Verdicts this daemon stored, by tree, base and gate settings: the local copy of the hub's
//! verdicts (see `hub::verdicts`), so that a landing finds a verdict while the hub cannot be
//! reached. The verdict is kept as JSON, which this module does not read.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::time::Timestamp;

use crate::Result;

/// Verdicts older than this many days are deleted, as at the hub.
pub const KEEP_DAYS: f64 = 30.0;

/// The verdict JSON stored under `tree`, `base` and `settings`, if any.
pub fn get(conn: &Connection, tree: &str, base: &str, settings: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT verdict FROM verdicts WHERE tree = ?1 AND base = ?2 AND settings = ?3",
            params![tree, base, settings],
            |row| row.get(0),
        )
        .optional()?)
}

/// Stores a verdict that passed at `passed_at`, replacing one under the same key, and deletes
/// every verdict that passed more than [`KEEP_DAYS`] days before `now`.
pub fn put(
    conn: &Connection,
    tree: &str,
    base: &str,
    settings: &str,
    verdict: &str,
    passed_at: Timestamp,
    now: Timestamp,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO verdicts (tree, base, settings, verdict, passed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![tree, base, settings, verdict, passed_at.to_string()],
    )?;
    let cutoff = now.plus_seconds(-KEEP_DAYS * 86_400.0);
    conn.execute(
        "DELETE FROM verdicts WHERE passed_at < ?1",
        params![cutoff.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn a_verdict_is_found_by_its_key_until_a_later_one_forgets_it() {
        let conn = open_in_memory().unwrap();
        let start: Timestamp = "2026-11-01T00:00:00.000Z".parse().unwrap();
        put(&conn, "t", "b", "s", "{}", start, start).unwrap();
        assert_eq!(get(&conn, "t", "b", "s").unwrap().as_deref(), Some("{}"));
        assert_eq!(get(&conn, "t2", "b", "s").unwrap(), None);
        assert_eq!(get(&conn, "t", "b2", "s").unwrap(), None);
        assert_eq!(get(&conn, "t", "b", "s2").unwrap(), None);
        let later = start.plus_seconds(31.0 * 86_400.0);
        put(&conn, "t2", "b", "s", "{}", later, later).unwrap();
        assert_eq!(get(&conn, "t", "b", "s").unwrap(), None);
        assert!(get(&conn, "t2", "b", "s").unwrap().is_some());
    }
}
