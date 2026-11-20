//! Verdicts that machines share, by tree, base and gate settings. The hub keeps each verdict
//! as the JSON the machine sent and never reads it.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::time::Timestamp;

use crate::Result;

/// Verdicts older than this many days are deleted.
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

/// Stores a verdict that passed at `passed_at`, replacing one under the same key.
pub fn put(
    conn: &Connection,
    tree: &str,
    base: &str,
    settings: &str,
    verdict: &str,
    passed_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO verdicts (tree, base, settings, verdict, passed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![tree, base, settings, verdict, passed_at],
    )?;
    Ok(())
}

/// Deletes verdicts that passed more than [`KEEP_DAYS`] days before `now`; returns how many.
pub fn prune(conn: &Connection, now: Timestamp) -> Result<u64> {
    let cutoff = now.plus_seconds(-KEEP_DAYS * 86_400.0);
    let deleted = conn.execute(
        "DELETE FROM verdicts WHERE passed_at < ?1",
        params![cutoff.to_string()],
    )?;
    Ok(deleted as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;

    #[test]
    fn a_verdict_is_found_by_tree_base_and_settings_and_old_ones_are_pruned() {
        let conn = open_in_memory().unwrap();
        let start: Timestamp = "2026-11-01T00:00:00.000Z".parse().unwrap();
        put(&conn, "t", "b", "s", "{}", &start.to_string()).unwrap();
        assert_eq!(get(&conn, "t", "b", "s").unwrap().as_deref(), Some("{}"));
        assert_eq!(get(&conn, "t2", "b", "s").unwrap(), None);
        assert_eq!(get(&conn, "t", "b2", "s").unwrap(), None);
        assert_eq!(get(&conn, "t", "b", "s2").unwrap(), None);
        assert_eq!(
            prune(&conn, start.plus_seconds(29.0 * 86_400.0)).unwrap(),
            0
        );
        assert_eq!(
            prune(&conn, start.plus_seconds(31.0 * 86_400.0)).unwrap(),
            1
        );
        assert_eq!(get(&conn, "t", "b", "s").unwrap(), None);
    }
}
