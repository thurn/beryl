//! The hub rows an efficiency store reads on a machine without the hub database.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::Serialize;

use super::rows::Record;
use crate::Result;

const METRIC_PAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize)]
pub struct FleetRecord {
    #[serde(flatten)]
    pub record: Record,
    pub machine_name: Option<String>,
    pub machine_cores: Option<i64>,
}

/// Command records in hub ID order, including records whose machine has no fleet row.
pub fn records(conn: &Connection, after: i64, limit: i64) -> Result<Vec<FleetRecord>> {
    let mut statement = conn.prepare(
        "SELECT r.id, r.machine, r.ticket, r.session, r.tree, r.verb, r.class, r.family,
                r.queued_s, r.wall_s, r.cpu_s, r.instructions, r.peak_mb, r.outcome,
                r.commit_sha, r.ignite, r.at, m.name, m.cores
         FROM records r LEFT JOIN machines m ON m.id = r.machine
         WHERE r.id > ?1 ORDER BY r.id LIMIT ?2",
    )?;
    let rows = statement.query_map(params![after, limit], |row| {
        Ok(FleetRecord {
            record: Record {
                id: row.get(0)?,
                machine: row.get(1)?,
                ticket: row.get(2)?,
                session: row.get(3)?,
                tree: row.get(4)?,
                verb: row.get(5)?,
                class: row.get(6)?,
                family: row.get(7)?,
                queued_s: row.get(8)?,
                wall_s: row.get(9)?,
                cpu_s: row.get(10)?,
                instructions: row.get(11)?,
                peak_mb: row.get(12)?,
                outcome: row.get(13)?,
                commit_sha: row.get(14)?,
                ignite: row.get(15)?,
                at: row.get(16)?,
            },
            machine_name: row.get(17)?,
            machine_cores: row.get(18)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Serialize)]
pub struct MetricRow {
    pub machine: String,
    pub machine_name: Option<String>,
    pub source: String,
    /// The original JSON line, as stored by the hub.
    pub data: String,
    pub at: String,
}

#[derive(Serialize)]
pub struct ExportHeader {
    pub machine: String,
    pub source: String,
    pub data: String,
}

pub struct MetricPage {
    pub rows: Vec<MetricRow>,
    pub headers: Vec<ExportHeader>,
    pub last_id: i64,
}

/// Pages through retained imports. The header for a machine and source may have arrived on
/// another page. Raw sources such as plan usage and findings have no export header.
pub fn metrics(conn: &mut Connection, after: i64, limit: i64) -> Result<MetricPage> {
    // Read the page and its headers against one snapshot without taking a write lock.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut page = MetricPage {
        rows: Vec::new(),
        headers: Vec::new(),
        last_id: after,
    };
    let mut statement = tx.prepare(
        "SELECT x.ingest_id, x.machine, m.name, x.source, x.data, x.at
         FROM metrics x LEFT JOIN machines m ON m.id = x.machine
         WHERE x.ingest_id > ?1 ORDER BY x.ingest_id LIMIT ?2",
    )?;
    let mut header = tx.prepare(
        "SELECT data FROM metrics WHERE machine = ?1 AND source = ?2 AND kind IS NULL
         AND CASE WHEN json_valid(data) THEN json_type(data, '$.export') IS NOT NULL END
         ORDER BY CAST(json_extract(data, '$.schema') AS INTEGER) DESC, ingest_id DESC LIMIT 1",
    )?;
    let mut sources = BTreeSet::new();
    let mut bytes = 0;
    let mut rows = statement.query(params![after, limit])?;
    while let Some(row) = rows.next()? {
        let entry = MetricRow {
            machine: row.get(1)?,
            machine_name: row.get(2)?,
            source: row.get(3)?,
            data: row.get(4)?,
            at: row.get(5)?,
        };
        let new_header: Option<String> =
            if sources.insert((entry.machine.clone(), entry.source.clone())) {
                header
                    .query_row(params![entry.machine, entry.source], |row| row.get(0))
                    .optional()?
            } else {
                None
            };
        // Stop before a page grows past 4 MiB of raw data and headers. Always send at least
        // one row so an unusually large accepted import cannot stall the cursor.
        bytes += entry.data.len() + new_header.as_ref().map_or(0, String::len);
        if bytes > METRIC_PAGE_BYTES && !page.rows.is_empty() {
            break;
        }
        if let Some(data) = new_header {
            page.headers.push(ExportHeader {
                machine: entry.machine.clone(),
                source: entry.source.clone(),
                data,
            });
        }
        page.last_id = row.get(0)?;
        page.rows.push(entry);
    }
    Ok(page)
}

/// ID, creation, claim and landing times, and whether this ticket repaired a regression.
type TicketTiming = (String, String, Option<String>, Option<String>, i64);
/// Commit, first seen time, verification time and state.
type TipTiming = (String, String, Option<String>, String);
/// Sequence, machine, ticket, kind and time.
type Handoff = (i64, String, String, String, String);
/// Machine, session and role.
type Worker = (String, String, String);

#[derive(Serialize)]
pub struct State {
    tickets: Vec<TicketTiming>,
    tips: Vec<TipTiming>,
    regressions: Vec<Regression>,
    handoffs: Vec<Handoff>,
    workers: Vec<Worker>,
}

#[derive(Serialize)]
struct Regression {
    check_name: String,
    ticket: String,
    first_bad: String,
    last_good: String,
    state: String,
    platform: Option<String>,
    culprit_ticket: Option<String>,
}

/// A compact, consistently ordered snapshot of the hub state used by efficiency reports.
pub fn state(conn: &mut Connection) -> Result<State> {
    // The report reads several tables from one snapshot without taking a write lock.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
    Ok(State {
        tickets: read(
            &tx,
            "WITH repairs(ticket) AS (
                 SELECT ticket FROM regressions UNION
                 SELECT ticket FROM handoffs WHERE kind = 'regression'
             )
             SELECT id, created_at, claimed_at, landed_at, repairs.ticket IS NOT NULL
             FROM tickets LEFT JOIN repairs ON repairs.ticket = tickets.id ORDER BY id",
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?,
        tips: read(
            &tx,
            "SELECT commit_sha, seen, verified, state FROM tips ORDER BY commit_sha",
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?,
        regressions: read(
            &tx,
            "SELECT check_name, ticket, first_bad, last_good, state, platform, culprit_ticket
             FROM regressions ORDER BY check_name",
            |r| {
                Ok(Regression {
                    check_name: r.get(0)?,
                    ticket: r.get(1)?,
                    first_bad: r.get(2)?,
                    last_good: r.get(3)?,
                    state: r.get(4)?,
                    platform: r.get(5)?,
                    culprit_ticket: r.get(6)?,
                })
            },
        )?,
        handoffs: read(
            &tx,
            "SELECT seq, machine, ticket, kind, at FROM handoffs ORDER BY seq",
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?,
        workers: read(
            &tx,
            "SELECT machine, session, role FROM workers ORDER BY machine, session",
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?,
    })
}

fn read<T>(
    conn: &Connection,
    sql: &str,
    row: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut statement = conn.prepare(sql)?;
    Ok(statement
        .query_map([], row)?
        .collect::<rusqlite::Result<_>>()?)
}
