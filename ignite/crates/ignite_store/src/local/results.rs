//! The machine's copy of the result cache, used first and while the hub is unreachable.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::local::rows;

/// The cached result of `check` on inputs hashing to `hash`. `platform` is empty for a portable
/// check.
pub fn get(
    conn: &Connection,
    check: &str,
    hash: &str,
    platform: &str,
) -> Result<Option<rows::CachedResult>> {
    Ok(conn
        .query_row(
            "SELECT check_name, hash, platform, outcome, record, at, output, machine FROM results \
             WHERE check_name = ?1 AND hash = ?2 AND platform = ?3",
            params![check, hash, platform],
            |row| {
                Ok(rows::CachedResult {
                    check_name: row.get(0)?,
                    hash: row.get(1)?,
                    platform: row.get(2)?,
                    outcome: row.get(3)?,
                    record: row.get(4)?,
                    at: row.get(5)?,
                    output: row.get(6)?,
                    machine: row.get(7)?,
                })
            },
        )
        .optional()?)
}

/// Stores the result, replacing one for the same check, hash and platform. That is how a result
/// gains the hub's record ID once the hub accepts it.
pub fn put(conn: &Connection, result: &rows::CachedResult) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO results \
         (check_name, hash, platform, outcome, record, at, output, machine) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            result.check_name,
            result.hash,
            result.platform,
            result.outcome,
            result.record,
            result.at,
            result.output,
            result.machine,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn result(platform: &str, outcome: &str) -> rows::CachedResult {
        rows::CachedResult {
            check_name: "rust:alpha::tests".to_owned(),
            hash: "abc123".to_owned(),
            platform: platform.to_owned(),
            outcome: outcome.to_owned(),
            record: None,
            at: "2026-11-18T00:00:00.000Z".to_owned(),
            output: Some("ok".to_owned()),
            machine: Some("m-1".to_owned()),
        }
    }

    #[test]
    fn results_are_keyed_by_check_hash_and_platform() {
        let conn = open_in_memory().unwrap();
        put(&conn, &result("", "pass")).unwrap();
        put(&conn, &result("macos", "fail")).unwrap();
        let get_outcome = |platform| {
            get(&conn, "rust:alpha::tests", "abc123", platform)
                .unwrap()
                .map(|r| r.outcome)
        };
        assert_eq!(get_outcome("").as_deref(), Some("pass"));
        assert_eq!(get_outcome("macos").as_deref(), Some("fail"));
        assert_eq!(get_outcome("windows"), None);
        assert_eq!(get(&conn, "rust:alpha::tests", "other", "").unwrap(), None);
    }

    #[test]
    fn putting_again_adds_the_record_id() {
        let conn = open_in_memory().unwrap();
        put(&conn, &result("", "pass")).unwrap();
        let mut accepted = result("", "pass");
        accepted.record = Some(77);
        put(&conn, &accepted).unwrap();
        assert_eq!(
            get(&conn, "rust:alpha::tests", "abc123", "").unwrap(),
            Some(accepted)
        );
    }
}
