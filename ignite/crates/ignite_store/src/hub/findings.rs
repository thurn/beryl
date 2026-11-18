//! Findings: what each improvement rule found, the ticket filed for it and when it closed, plus
//! each exploration family's finds by day and the rebuild counts.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::rows;

/// A rule files nothing for a subject until this long after closing a ticket for it.
pub const REFILE_DAYS: f64 = 30.0;

/// Adds the finding for its (rule, subject), or replaces the row that is there.
pub fn upsert(conn: &Connection, finding: &rows::Finding) -> Result<()> {
    conn.execute(
        "INSERT INTO findings (rule, subject, unit, cost, saving, ticket, closed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(rule, subject) DO UPDATE SET unit = excluded.unit, cost = excluded.cost,
           saving = excluded.saving, ticket = excluded.ticket, closed_at = excluded.closed_at",
        params![
            finding.rule,
            finding.subject,
            finding.unit,
            finding.cost,
            finding.saving,
            finding.ticket,
            finding.closed_at,
        ],
    )?;
    Ok(())
}

/// Marks the findings whose ticket is `ticket` closed at `now`.
pub fn close_ticket(conn: &Connection, ticket: &str, now: Timestamp) -> Result<()> {
    conn.execute(
        "UPDATE findings SET closed_at = ?2 WHERE ticket = ?1",
        params![ticket, now.to_string()],
    )?;
    Ok(())
}

/// Whether the rule may file for the subject: it has no open ticket for it, and none closed
/// within the last 30 days.
pub fn may_file(conn: &Connection, rule: &str, subject: &str, now: Timestamp) -> Result<bool> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT ticket, closed_at FROM findings WHERE rule = ?1 AND subject = ?2",
            params![rule, subject],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((ticket, closed_at)) = row else {
        return Ok(true);
    };
    let Some(closed_at) = closed_at else {
        // A finding with a ticket that is still open is not filed again.
        return Ok(ticket.is_none());
    };
    let closed =
        Timestamp::parse(&closed_at).map_err(|e| crate::StoreError::Invalid(e.to_string()))?;
    Ok(now.seconds_since(closed) >= REFILE_DAYS * 86_400.0)
}

/// The findings whose ticket has not closed, ordered by rule and subject.
pub fn open(conn: &Connection) -> Result<Vec<rows::Finding>> {
    let mut statement = conn.prepare(
        "SELECT rule, subject, unit, cost, saving, ticket, closed_at FROM findings
         WHERE closed_at IS NULL ORDER BY rule, subject",
    )?;
    let findings = statement
        .query_map([], |row| {
            Ok(rows::Finding {
                rule: row.get(0)?,
                subject: row.get(1)?,
                unit: row.get(2)?,
                cost: row.get(3)?,
                saving: row.get(4)?,
                ticket: row.get(5)?,
                closed_at: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(findings)
}

/// Adds bugs found and instructions spent by an exploration family on one day (`YYYY-MM-DD`).
pub fn add_exploration(
    conn: &Connection,
    family: &str,
    day: &str,
    bugs: u32,
    instructions: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO explore (family, day, bugs, instructions) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(family, day) DO UPDATE SET bugs = bugs + excluded.bugs,
           instructions = instructions + excluded.instructions",
        params![family, day, i64::from(bugs), instructions],
    )?;
    Ok(())
}

/// The exploration rows from `since_day` (inclusive, `YYYY-MM-DD`) on, ordered by family and
/// day.
pub fn exploration_since(conn: &Connection, since_day: &str) -> Result<Vec<rows::Explore>> {
    let mut statement = conn.prepare(
        "SELECT family, day, bugs, instructions FROM explore WHERE day >= ?1
         ORDER BY family, day",
    )?;
    let days = statement
        .query_map([since_day], |row| {
            Ok(rows::Explore {
                family: row.get(0)?,
                day: row.get(1)?,
                bugs: row.get(2)?,
                instructions: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(days)
}

/// Stores the rebuild count for (name, commit), replacing an earlier measurement.
pub fn record_rebuild(conn: &Connection, rebuild: &rows::Rebuild) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO rebuilds (name, commit_sha, instructions, crates)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            rebuild.name,
            rebuild.commit_sha,
            rebuild.instructions,
            rebuild.crates
        ],
    )?;
    Ok(())
}

/// The rebuild count measured for `name` at `commit`.
pub fn rebuild_at(conn: &Connection, name: &str, commit: &str) -> Result<Option<rows::Rebuild>> {
    Ok(conn
        .query_row(
            "SELECT name, commit_sha, instructions, crates FROM rebuilds
             WHERE name = ?1 AND commit_sha = ?2",
            params![name, commit],
            |row| {
                Ok(rows::Rebuild {
                    name: row.get(0)?,
                    commit_sha: row.get(1)?,
                    instructions: row.get(2)?,
                    crates: row.get(3)?,
                })
            },
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;

    fn finding(ticket: Option<&str>, closed_at: Option<&str>) -> rows::Finding {
        rows::Finding {
            rule: "slow-crate".into(),
            subject: "beryl_session".into(),
            unit: "core-hours".into(),
            cost: 38.2,
            saving: 15.3,
            ticket: ticket.map(Into::into),
            closed_at: closed_at.map(Into::into),
        }
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn a_new_subject_may_be_filed_and_an_open_ticket_blocks_it() {
        let conn = hub::open_in_memory().unwrap();
        let now = at("2026-11-17T00:00:00.000Z");
        assert!(may_file(&conn, "slow-crate", "beryl_session", now).unwrap());
        upsert(&conn, &finding(None, None)).unwrap();
        assert!(may_file(&conn, "slow-crate", "beryl_session", now).unwrap());
        upsert(&conn, &finding(Some("Tm41q"), None)).unwrap();
        assert!(!may_file(&conn, "slow-crate", "beryl_session", now).unwrap());
        assert_eq!(open(&conn).unwrap().len(), 1);
    }

    #[test]
    fn a_finding_is_blocked_for_30_days_after_its_ticket_closes() {
        let conn = hub::open_in_memory().unwrap();
        upsert(&conn, &finding(Some("Tm41q"), None)).unwrap();
        close_ticket(&conn, "Tm41q", at("2026-11-01T12:00:00.000Z")).unwrap();
        assert!(open(&conn).unwrap().is_empty());
        let blocked = |now| may_file(&conn, "slow-crate", "beryl_session", at(now)).unwrap();
        assert!(!blocked("2026-11-01T12:00:00.000Z"));
        assert!(!blocked("2026-12-01T11:59:59.999Z"));
        assert!(blocked("2026-12-01T12:00:00.000Z"));
        // Another subject of the same rule is not blocked.
        assert!(
            may_file(
                &conn,
                "slow-crate",
                "beryl_kernel",
                at("2026-11-02T00:00:00.000Z")
            )
            .unwrap()
        );
    }

    #[test]
    fn upsert_replaces_the_row_for_one_rule_and_subject() {
        let conn = hub::open_in_memory().unwrap();
        upsert(&conn, &finding(None, None)).unwrap();
        let mut bigger = finding(None, None);
        bigger.saving = 20.0;
        upsert(&conn, &bigger).unwrap();
        let rows = open(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].saving - 20.0).abs() < 1e-9);
    }

    #[test]
    fn exploration_adds_up_by_family_and_day() {
        let conn = hub::open_in_memory().unwrap();
        add_exploration(&conn, "fuzz", "2026-11-16", 1, 5.0e9).unwrap();
        add_exploration(&conn, "fuzz", "2026-11-16", 2, 1.0e9).unwrap();
        add_exploration(&conn, "fuzz", "2026-11-17", 0, 2.0e9).unwrap();
        add_exploration(&conn, "cards", "2026-10-01", 4, 1.0e9).unwrap();
        let days = exploration_since(&conn, "2026-11-01").unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!((days[0].bugs, days[0].day.as_str()), (3, "2026-11-16"));
        assert!((days[0].instructions - 6.0e9).abs() < 1.0);
    }

    #[test]
    fn rebuild_counts_are_kept_by_name_and_commit() {
        let conn = hub::open_in_memory().unwrap();
        let rebuild = |commit: &str, instructions| rows::Rebuild {
            name: "kernel-body".into(),
            commit_sha: commit.into(),
            instructions,
            crates: r#"{"beryl_kernel": 1.0}"#.into(),
        };
        record_rebuild(&conn, &rebuild("aaaa", 1.0e12)).unwrap();
        record_rebuild(&conn, &rebuild("bbbb", 2.0e12)).unwrap();
        record_rebuild(&conn, &rebuild("aaaa", 1.5e12)).unwrap();
        assert_eq!(
            rebuild_at(&conn, "kernel-body", "aaaa").unwrap(),
            Some(rebuild("aaaa", 1.5e12))
        );
        assert_eq!(rebuild_at(&conn, "kernel-body", "cccc").unwrap(), None);
    }
}
