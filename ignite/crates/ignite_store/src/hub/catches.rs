//! Catches read from Ignite's own records, for `ignite gate replay`. Reads only.
//!
//! A catch is a failure of a regression check that a later change made pass again (see "Scores" in
//! `docs/ignite.md`). The hub holds two kinds:
//!
//! - **Gate catches**: a `fail` in the result cache (`results`) whose check and input hash have
//!   no flaky record, made by a command for a ticket that landed afterwards. Landing needs a
//!   gate that passed, so the ticket's change was edited until the check passed or no longer
//!   ran. A result names no ticket (its `record` is 0 when the daemon did not know the hub's
//!   record ID), so the command is found from the records: the record of the result's machine
//!   and the check's family, for a ticket, nearest in time to the result within [`WINDOW_S`],
//!   preferring a record whose outcome is `fail`. One catch is counted per ticket and check,
//!   at its first failure.
//! - **Tip catches**: a regression (`regressions`) in state `fixed`: the check failed at a tip
//!   and a later change made it pass. Its time is when its ticket was filed, and its change is
//!   the landed ticket whose commits include the first bad commit. The hub keeps one regression
//!   per check, so a check that regressed twice counts once.
//!
//! A catch's areas are the areas of the files its ticket landed (`tickets.landed`), or `[""]`
//! when they are unknown. Checks the hub lists as exploration, or does not list, are left out.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

use ignite_core::inputs::{self, CrateGraph};
use ignite_core::time::Timestamp;

use crate::Result;
use crate::import::runlog::Catch;

/// How far apart in seconds a result and the record of the command that made it may be.
pub const WINDOW_S: f64 = 3600.0;

#[derive(Deserialize, Default)]
struct Landed {
    #[serde(default)]
    commits: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
}

/// A failure in the result cache: its check, the check's family, the machine, the record it
/// names (0 for none) and when it was stored.
type Failure = (String, String, String, i64, Timestamp);

/// One landed ticket: when it landed and what.
struct Landing {
    at: Timestamp,
    landed: Landed,
}

/// Every catch since `since`, oldest first, with areas named by `graph`.
pub fn since(conn: &Connection, since: Timestamp, graph: &CrateGraph) -> Result<Vec<Catch>> {
    let landings = landings(conn)?;
    let areas = |ticket: Option<&String>| -> Vec<String> {
        let files = ticket
            .and_then(|t| landings.get(t))
            .map(|l| &l.landed.files[..])
            .unwrap_or_default();
        let found: BTreeSet<String> = files
            .iter()
            .map(|path| inputs::area(path, graph).to_string())
            .collect();
        if found.is_empty() {
            vec![String::new()]
        } else {
            found.into_iter().collect()
        }
    };
    let mut found = Vec::new();
    // The first failure of each check for each ticket.
    let mut first: BTreeMap<(String, String), Timestamp> = BTreeMap::new();
    for (check, family, machine, record, at) in gate_failures(conn, since)? {
        let Some(ticket) = ticket_of(conn, &machine, &family, record, at)? else {
            continue;
        };
        let Some(landing) = landings.get(&ticket) else {
            continue;
        };
        if landing.at < at {
            continue;
        }
        let entry = first.entry((ticket, check)).or_insert(at);
        *entry = (*entry).min(at);
    }
    for ((ticket, check), at) in first {
        found.push(Catch {
            check,
            at,
            areas: areas(Some(&ticket)),
        });
    }
    for (check, first_bad, at) in fixed_regressions(conn, since)? {
        // A commit landed again by a later ticket belongs to the later one.
        let culprit = landings
            .iter()
            .filter(|(_, l)| l.landed.commits.contains(&first_bad))
            .max_by_key(|(_, l)| l.at)
            .map(|(ticket, _)| ticket);
        found.push(Catch {
            check,
            at,
            areas: areas(culprit),
        });
    }
    found.sort_by(|a, b| (a.at, &a.check).cmp(&(b.at, &b.check)));
    Ok(found)
}

fn timestamp(text: &str) -> Option<Timestamp> {
    Timestamp::parse(text).ok()
}

/// Every landed ticket, by ID.
fn landings(conn: &Connection) -> Result<BTreeMap<String, Landing>> {
    let mut statement = conn.prepare(
        "SELECT id, landed, landed_at FROM tickets
         WHERE landed IS NOT NULL AND landed_at IS NOT NULL",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, landed, at)| {
            let landing = Landing {
                at: timestamp(&at)?,
                landed: serde_json::from_str(&landed).unwrap_or_default(),
            };
            Some((id, landing))
        })
        .collect())
}

/// Failures of regression checks since `since` that no flaky record explains: check, family,
/// machine, record and time.
fn gate_failures(conn: &Connection, since: Timestamp) -> Result<Vec<Failure>> {
    let mut statement = conn.prepare(
        "SELECT r.check_name, c.family, r.machine, r.record, r.at
         FROM results r JOIN checks c ON c.name = r.check_name
         WHERE r.outcome = 'fail' AND c.kind = 'regression' AND r.at >= ?1
           AND NOT EXISTS (SELECT 1 FROM flaky f
                           WHERE f.check_name = r.check_name AND f.hash = r.hash)",
    )?;
    let rows = statement
        .query_map(params![since.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(check, family, machine, record, at)| {
            Some((check, family, machine, record, timestamp(&at)?))
        })
        .collect())
}

/// The ticket of the command that made a result: the record it names, or else the nearest
/// record of the machine and family for a ticket within [`WINDOW_S`] (see the module header).
fn ticket_of(
    conn: &Connection,
    machine: &str,
    family: &str,
    record: i64,
    at: Timestamp,
) -> Result<Option<String>> {
    if record != 0 {
        let named: Option<Option<String>> = conn
            .query_row(
                "SELECT ticket FROM records WHERE id = ?1",
                params![record],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(ticket) = named {
            return Ok(ticket);
        }
    }
    let mut statement = conn.prepare(
        "SELECT ticket, outcome, at FROM records
         WHERE machine = ?1 AND family = ?2 AND ticket IS NOT NULL AND at >= ?3 AND at <= ?4",
    )?;
    let low = at.plus_seconds(-WINDOW_S).to_string();
    let high = at.plus_seconds(WINDOW_S).to_string();
    let rows = statement
        .query_map(params![machine, family, low, high], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(ticket, outcome, when)| {
            let distance = timestamp(&when)?.seconds_since(at).abs();
            Some((outcome.as_deref() != Some("fail"), distance, ticket))
        })
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, _, ticket)| ticket))
}

/// Fixed regressions of regression checks whose tickets were filed since `since`: check, first
/// bad commit and when the ticket was filed.
fn fixed_regressions(
    conn: &Connection,
    since: Timestamp,
) -> Result<Vec<(String, String, Timestamp)>> {
    let mut statement = conn.prepare(
        "SELECT g.check_name, g.first_bad, t.created_at
         FROM regressions g
         JOIN tickets t ON t.id = g.ticket
         JOIN checks c ON c.name = g.check_name
         WHERE g.state = 'fixed' AND c.kind = 'regression' AND t.created_at >= ?1",
    )?;
    let rows = statement
        .query_map(params![since.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(check, first_bad, at)| Some((check, first_bad, timestamp(&at)?)))
        .collect())
}

#[cfg(test)]
#[path = "catches_tests.rs"]
mod tests;
