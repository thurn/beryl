//! Check outcomes are written with the ticket close, while the ticket still owns the same
//! regression and flaky records. A reopened ticket can record a new outcome on its next close.

use rusqlite::{Connection, params};

use crate::hub::flaky_triage;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::{CheckFix, FixOutcome, FixSource};
use ignite_core::time::Timestamp;

use crate::hub::{findings, rows};
use crate::{Result, StoreError};

/// The classification and tip of a landing closing a ticket in the current transaction.
/// A later close after reopening the ticket must ignore this attempt's landing.
pub struct LandingFix {
    pub outcome: FixOutcome,
    pub commit: Option<Commit>,
    /// Commits the same ticket landed before this close, for older regression reports that
    /// did not record which ticket landed their culprit.
    pub prior_commits: Vec<Commit>,
}

/// Records one outcome per check on the ticket, and credits exploration families for a
/// product fix using that same classification.
pub(super) fn record_close(
    conn: &Connection,
    ticket: &str,
    close_seq: i64,
    now: Timestamp,
    landing: Option<&LandingFix>,
) -> Result<()> {
    let outcome = landing.map_or(FixOutcome::None, |fix| fix.outcome);
    let commit = landing
        .and_then(|fix| fix.commit.as_ref())
        .map(Commit::as_str);
    let prior_commits = landing.map_or(&[][..], |fix| fix.prior_commits.as_slice());
    let prior_commits = serde_json::to_string(prior_commits)
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    conn.execute(
        "INSERT INTO check_fix_outcomes
            (close_seq, check_name, ticket, at, outcome, source, first_bad, fixing_commit,
             failure_output, remedy_ticket)
         SELECT ?1, r.check_name, ?2, ?3, ?4, 'regression', r.first_bad, ?5,
             CASE WHEN ?4 = 'test-only' THEN r.failure_output END,
             CASE WHEN ?4 = 'test-only' AND ?5 IS NOT NULL AND NOT EXISTS (
                 SELECT 1 FROM json_each(?6) WHERE value = r.first_bad
             ) AND coalesce(r.culprit_ticket, '') != ?2 THEN '' END
         FROM regressions r WHERE r.ticket = ?2
         UNION ALL
         SELECT ?1, check_name, ?2, ?3, ?4, 'flaky', NULL, ?5, NULL, NULL
         FROM (
             SELECT check_name FROM flaky WHERE filed = ?2
             UNION
             SELECT listed.value FROM tickets, json_each(tickets.metadata, '$.ignite_checks') listed
             WHERE tickets.id = ?2 AND listed.type = 'text'
         ) WHERE EXISTS (
             SELECT 1 FROM tickets, json_each(tickets.labels)
             WHERE tickets.id = ?2 AND json_each.value = 'flaky'
               AND json_extract(tickets.metadata, '$.ignite_fault.category') IS NOT ?7
         )",
        params![
            close_seq,
            ticket,
            now.to_string(),
            outcome.as_str(),
            commit,
            prior_commits,
            flaky_triage::CATEGORY,
        ],
    )?;
    if outcome == FixOutcome::Product {
        let mut families = conn.prepare(
            "SELECT DISTINCT substr(json_each.value, 10) FROM tickets, json_each(tickets.labels)
             WHERE tickets.id = ?1 AND json_each.value GLOB 'found-by:?*'",
        )?;
        let day = now.day();
        for family in families.query_map([ticket], |row| row.get::<_, String>(0))? {
            findings::add_exploration(conn, &family?, &day, 1, 0.0)?;
        }
    }
    Ok(())
}

/// The window that the hub reports to `ignite why` for an individual check.
pub const HISTORY_DAYS: f64 = 30.0;

/// Each close from `start` through `end`, most recent first. Pass a check to read only its
/// history, or leave it unset to read the outcomes across checks for scoring or upkeep.
pub fn between(
    conn: &Connection,
    start: Timestamp,
    end: Timestamp,
    check: Option<&CheckName>,
) -> Result<Vec<CheckFix>> {
    let all = format!("{SELECT} WHERE at BETWEEN ?1 AND ?2");
    let start = start.to_string();
    let end = end.to_string();
    match check {
        Some(check) => read(
            conn,
            &format!("{all} AND check_name = ?3 {ORDER}"),
            params![start, end, check.as_str()],
        ),
        None => read(conn, &format!("{all} {ORDER}"), params![start, end]),
    }
}

/// All recorded product fixes, for scores and threshold replay.
pub fn products(conn: &Connection) -> Result<Vec<CheckFix>> {
    read(
        conn,
        &format!("{SELECT} WHERE outcome = 'product' {ORDER}"),
        [],
    )
}

const SELECT: &str =
    "SELECT close_seq, check_name, ticket, at, outcome, source, first_bad, fixing_commit,
            failure_output, remedy_ticket
     FROM check_fix_outcomes";
const ORDER: &str = "ORDER BY at DESC, close_seq DESC, check_name, source";

fn read(conn: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<Vec<CheckFix>> {
    let mut query = conn.prepare(sql)?;
    let read = |row: &rusqlite::Row<'_>| {
        Ok(rows::CheckFixOutcome {
            close_seq: row.get(0)?,
            check_name: row.get(1)?,
            ticket: row.get(2)?,
            at: row.get(3)?,
            outcome: row.get(4)?,
            source: row.get(5)?,
            first_bad: row.get(6)?,
            fixing_commit: row.get(7)?,
            failure_output: row.get(8)?,
            remedy_ticket: row.get(9)?,
        })
    };
    let rows = query.query_map(args, read)?;
    rows.map(|row| decode(&row?)).collect()
}

fn decode(row: &rows::CheckFixOutcome) -> Result<CheckFix> {
    let outcome = match row.outcome.as_str() {
        "product" => FixOutcome::Product,
        "test-only" => FixOutcome::TestOnly,
        "none" => FixOutcome::None,
        other => {
            return Err(StoreError::Invalid(format!(
                "unknown fix outcome {other:?}"
            )));
        }
    };
    let source = match row.source.as_str() {
        "regression" => FixSource::Regression,
        "flaky" => FixSource::Flaky,
        other => return Err(StoreError::Invalid(format!("unknown fix source {other:?}"))),
    };
    Ok(CheckFix {
        check: row.check_name.parse().map_err(invalid)?,
        ticket: row.ticket.parse().map_err(invalid)?,
        at: row.at.parse().map_err(invalid)?,
        outcome,
        source,
        first_bad: row
            .first_bad
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(invalid)?,
        fixing_commit: row
            .fixing_commit
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(invalid)?,
    })
}

fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(error.to_string())
}
