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

/// How long a session's latest outcome of a check is kept: a day. The dispatcher looks back at
/// most 2 hours (see `dispatchd_shared` in the daemon).
const SIGHTINGS_KEPT_S: f64 = 24.0 * 60.0 * 60.0;

/// Records `sighting` as its session's latest outcome of its check, replacing the one before,
/// and deletes sightings older than a day. A pass clears `tip_unknown`.
pub fn note_sighting(conn: &Connection, sighting: &rows::CheckSighting) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO check_sightings \
         (session, check_name, verb, outcome, hash, first_line, tip_unknown, at, head) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            sighting.session,
            sighting.check_name,
            sighting.verb,
            sighting.outcome,
            sighting.hash,
            sighting.first_line,
            sighting.tip_unknown,
            sighting.at,
            sighting.head,
        ],
    )?;
    if let Ok(at) = ignite_core::time::Timestamp::parse(&sighting.at) {
        let cutoff = at.plus_seconds(-SIGHTINGS_KEPT_S);
        conn.execute(
            "DELETE FROM check_sightings WHERE at < ?1",
            params![cutoff.to_string()],
        )?;
    }
    Ok(())
}

/// Marks `session`'s latest failure of `check` as one whose result at the tip could not be found
/// or run, which is what a refused landing reports (see `ignite guide blocked`).
pub fn mark_tip_unknown(conn: &Connection, session: &str, check: &str) -> Result<()> {
    conn.execute(
        "UPDATE check_sightings SET tip_unknown = 1 \
         WHERE session = ?1 AND check_name = ?2 AND outcome = 'fail'",
        params![session, check],
    )?;
    Ok(())
}

/// Every session's latest outcome of every check recorded at or after `since`, oldest first.
pub fn sightings_since(conn: &Connection, since: &str) -> Result<Vec<rows::CheckSighting>> {
    let mut statement = conn.prepare(
        "SELECT session, check_name, verb, outcome, hash, first_line, tip_unknown, at, head \
         FROM check_sightings WHERE at >= ?1 ORDER BY at",
    )?;
    let rows = statement.query_map(params![since], |row| {
        Ok(rows::CheckSighting {
            session: row.get(0)?,
            check_name: row.get(1)?,
            verb: row.get(2)?,
            outcome: row.get(3)?,
            hash: row.get(4)?,
            first_line: row.get(5)?,
            tip_unknown: row.get(6)?,
            at: row.get(7)?,
            head: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
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

    fn sighting(session: &str, outcome: &str, at: &str) -> rows::CheckSighting {
        rows::CheckSighting {
            session: session.to_owned(),
            check_name: "rust:alpha::tests".to_owned(),
            verb: "check".to_owned(),
            outcome: outcome.to_owned(),
            hash: Some("abc123".to_owned()),
            first_line: Some("assertion failed".to_owned()),
            tip_unknown: 0,
            at: at.to_owned(),
            head: Some("abc123".to_owned()),
        }
    }

    #[test]
    fn a_session_keeps_only_its_latest_outcome_of_a_check() {
        let conn = open_in_memory().unwrap();
        note_sighting(&conn, &sighting("s1", "fail", "2026-11-18T00:00:00.000Z")).unwrap();
        note_sighting(&conn, &sighting("s2", "fail", "2026-11-18T00:01:00.000Z")).unwrap();
        mark_tip_unknown(&conn, "s2", "rust:alpha::tests").unwrap();
        note_sighting(&conn, &sighting("s1", "pass", "2026-11-18T00:02:00.000Z")).unwrap();
        let all = sightings_since(&conn, "2026-11-17T00:00:00.000Z").unwrap();
        let summary: Vec<(&str, &str, i64)> = all
            .iter()
            .map(|s| (s.session.as_str(), s.outcome.as_str(), s.tip_unknown))
            .collect();
        assert_eq!(summary, [("s2", "fail", 1), ("s1", "pass", 0)]);
        let later = sightings_since(&conn, "2026-11-18T00:01:30.000Z").unwrap();
        assert_eq!(later.len(), 1);
    }

    #[test]
    fn sightings_older_than_a_day_are_deleted() {
        let conn = open_in_memory().unwrap();
        note_sighting(&conn, &sighting("s1", "fail", "2026-11-18T00:00:00.000Z")).unwrap();
        note_sighting(&conn, &sighting("s2", "fail", "2026-11-19T01:00:00.000Z")).unwrap();
        let all = sightings_since(&conn, "").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].session, "s2");
    }
}
