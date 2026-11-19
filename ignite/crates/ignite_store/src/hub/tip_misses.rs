//! Checks tip verification missed: what the hub knows of tip verification at a tip and the tips
//! after it, and the record of each miss (see "Tip verification" in `docs/ignite.md`).
//!
//! A landing whose gate finds a check failing at the `staging` tip it was rebased onto reports
//! the check as a regression. Tip verification should have found it first, so the hub looks up
//! what tip verification did at that tip and later ones ([`evidence`]) and records why it
//! missed the check ([`record`]). "Later" means first seen by the hub later: the hub learns of a
//! tip when a machine claims a batch at it or reports its verdict, and knows no commit order.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::time::Timestamp;
use ignite_core::tipround::RoundBudget;

use crate::hub::rows::TipMiss;
use crate::{Result, StoreError};

/// What tip verification did at a tip and at the tips the hub saw after it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    /// The first of those tips at which a tip verification round passed the check.
    pub passed_at: Option<String>,
    /// Every finished round at those tips, oldest tip first: its tip and how it split its
    /// checks, `None` from a daemon built before the budget.
    pub rounds: Vec<(String, Option<RoundBudget>)>,
}

/// How many finished rounds [`evidence`] reads; more than a day's worth after a missed check
/// would say nothing the first ones do not.
const ROUNDS_READ: u32 = 50;

/// What tip verification did about `check` at `tip` and the tips seen after it. A tip the hub
/// has never seen has no evidence: no round has claimed a batch at it.
pub fn evidence(conn: &Connection, check: &str, tip: &str) -> Result<Evidence> {
    let seen: Option<String> = conn
        .query_row(
            "SELECT seen FROM tips WHERE commit_sha = ?1",
            params![tip],
            |row| row.get(0),
        )
        .optional()?;
    let Some(seen) = seen else {
        return Ok(Evidence::default());
    };
    // A result's record is the command that produced it; a tip round's commands run in the
    // `tip` class at the round's tip.
    let passed_at: Option<String> = conn
        .query_row(
            "SELECT r.commit_sha FROM results p
             JOIN records r ON r.id = p.record
             JOIN tips t ON t.commit_sha = r.commit_sha
             WHERE p.check_name = ?1 AND p.outcome = 'pass' AND r.class = 'tip' AND t.seen >= ?2
             ORDER BY t.seen, r.commit_sha LIMIT 1",
            params![check, seen],
            |row| row.get(0),
        )
        .optional()?;
    let mut select = conn.prepare(
        "SELECT r.commit_sha, r.budget FROM tip_rounds r
         JOIN tips t ON t.commit_sha = r.commit_sha
         WHERE t.seen >= ?1 ORDER BY t.seen, r.at, r.id LIMIT ?2",
    )?;
    let rows = select
        .query_map(params![seen, ROUNDS_READ], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let rounds = rows
        .into_iter()
        .map(|(commit, budget)| {
            let budget = budget
                .map(|json| serde_json::from_str(&json))
                .transpose()
                .map_err(|e| StoreError::Invalid(format!("a round's budget: {e}")))?;
            Ok((commit, budget))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Evidence { passed_at, rounds })
}

/// Records a miss. Returns `false`, changing nothing, when the check's miss at that tip is
/// already recorded.
pub fn record(conn: &Connection, miss: &TipMiss) -> Result<bool> {
    let added = conn.execute(
        "INSERT OR IGNORE INTO tip_misses (check_name, family, tip, kind, reason, ticket, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            miss.check_name,
            miss.family,
            miss.tip,
            miss.kind,
            miss.reason,
            miss.ticket,
            miss.at
        ],
    )?;
    Ok(added > 0)
}

/// The misses recorded since `since`, of `family` alone when one is named, oldest first.
pub fn since(conn: &Connection, family: Option<&str>, since: Timestamp) -> Result<Vec<TipMiss>> {
    let mut select = conn.prepare(
        "SELECT check_name, family, tip, kind, reason, ticket, at FROM tip_misses
         WHERE at >= ?1 AND (?2 IS NULL OR family = ?2) ORDER BY at, check_name",
    )?;
    let misses = select
        .query_map(params![since.to_string(), family], |row| {
            Ok(TipMiss {
                check_name: row.get(0)?,
                family: row.get(1)?,
                tip: row.get(2)?,
                kind: row.get(3)?,
                reason: row.get(4)?,
                ticket: row.get(5)?,
                at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(misses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;

    fn miss(check: &str, tip: &str, at: &str) -> TipMiss {
        TipMiss {
            check_name: check.to_owned(),
            family: "unit".to_owned(),
            tip: tip.to_owned(),
            kind: "not-run".to_owned(),
            reason: "reason".to_owned(),
            ticket: "Tabc1".to_owned(),
            at: at.to_owned(),
        }
    }

    #[test]
    fn a_miss_is_recorded_once_and_read_back_by_family_and_time() {
        let conn = hub::open_in_memory().unwrap();
        assert!(record(&conn, &miss("unit:a", "abcd", "2026-11-19T08:00:00Z")).unwrap());
        assert!(!record(&conn, &miss("unit:a", "abcd", "2026-11-19T09:00:00Z")).unwrap());
        assert!(record(&conn, &miss("unit:b", "abcd", "2026-11-19T10:00:00Z")).unwrap());
        let later: Timestamp = "2026-11-19T09:00:00Z".parse().unwrap();
        let read = since(&conn, Some("unit"), later).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].check_name, "unit:b");
        assert!(
            since(&conn, Some("rust"), Timestamp::EPOCH)
                .unwrap()
                .is_empty()
        );
        assert_eq!(since(&conn, None, Timestamp::EPOCH).unwrap().len(), 2);
    }

    #[test]
    fn a_tip_the_hub_never_saw_has_no_evidence() {
        let conn = hub::open_in_memory().unwrap();
        assert_eq!(
            evidence(&conn, "unit:a", "abcd").unwrap(),
            Evidence::default()
        );
    }
}
