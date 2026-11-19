//! Express runs that passed, by the git tree they checked and the `[express]` section they ran.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// Records that the express commands described by `config` passed on `tree_id` at `at`, and
/// forgets every pass from before `forget_before`.
pub fn record(
    conn: &Connection,
    tree_id: &str,
    config: &str,
    at: &str,
    forget_before: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO express_passes (tree_id, config, at) VALUES (?1, ?2, ?3)",
        params![tree_id, config, at],
    )?;
    conn.execute("DELETE FROM express_passes WHERE at < ?1", [forget_before])?;
    Ok(())
}

/// When the commands described by `config` last passed on `tree_id`, if that was at or after
/// `since`.
pub fn passed_at(
    conn: &Connection,
    tree_id: &str,
    config: &str,
    since: &str,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT at FROM express_passes WHERE tree_id = ?1 AND config = ?2 AND at >= ?3",
            params![tree_id, config, since],
            |row| row.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn a_pass_is_found_for_its_tree_and_section_until_it_expires() {
        let conn = open_in_memory().unwrap();
        record(
            &conn,
            "t1",
            "c1",
            "2026-11-18T10:00:00Z",
            "2026-11-18T09:00:00Z",
        )
        .unwrap();
        let since = "2026-11-18T10:00:00Z";
        assert!(passed_at(&conn, "t1", "c1", since).unwrap().is_some());
        assert_eq!(passed_at(&conn, "t2", "c1", since).unwrap(), None);
        assert_eq!(passed_at(&conn, "t1", "c2", since).unwrap(), None);
        assert_eq!(
            passed_at(&conn, "t1", "c1", "2026-11-18T10:00:01Z").unwrap(),
            None
        );
        // A later record forgets the expired one.
        record(
            &conn,
            "t2",
            "c1",
            "2026-11-18T12:00:00Z",
            "2026-11-18T11:00:00Z",
        )
        .unwrap();
        assert_eq!(
            passed_at(&conn, "t1", "c1", "2000-01-01T00:00:00Z").unwrap(),
            None
        );
    }
}
