//! The `metrics` table: the efficiency rows each machine's daemon posts to
//! `POST /metrics/import`, and how long the hub keeps them.
//!
//! The readers use rows retained for [`KEEP_DAYS`] days:
//!
//! - the hub's efficiency rule reads `findings` rows of the last 14 days, by `source` and `at`;
//! - the hub's backlog ratios read `window_dollars` rows of the last week, by `kind` and `at`;
//! - the efficiency dashboard on the hub's machine imports every row that arrived since its
//!   last import, by `at`, which it does many times a day;
//! - the private efficiency API pages through rows by `ingest_id`, so rows at the same
//!   arrival time may be read on separate pages.
//!
//! On 2026-11-20 the table held 1.5 million rows from two days, 3.3 GB, most of them rows the
//! dashboard rebuilds from the hub's own records ([`REBUILT_KINDS`]), and the readers scanned it
//! whole; every hub write waited on `database is locked` for minutes. The daemon no longer posts
//! those rows, upkeep deletes rows older than [`KEEP_DAYS`] in small batches
//! ([`delete_expired`]), and the readers have indexes on `(source, at)` and `(kind, at)`.

use rusqlite::{Connection, Transaction, params};

use ignite_core::time::Timestamp;

use crate::Result;

/// How long a row is kept after it arrived: longer than any reader looks back.
pub const KEEP_DAYS: f64 = 30.0;

/// The most rows one deletion removes, so that it holds the write lock for milliseconds.
pub const DELETE_BATCH: i64 = 500;

/// The most batches one call of [`delete_expired`] runs, each in its own transaction.
pub const DELETE_BATCHES: usize = 20;

/// The export tables the efficiency dashboard on the hub's machine fills from the hub's own
/// `records` and `compile_units` for every machine, so the copies a machine exports are
/// redundant there. The daemon leaves them out of its posts (`ignite_daemon::metrics`).
pub const REBUILT_KINDS: [&str; 7] = [
    "compile_timings",
    "ignite_records",
    "leases",
    "cpu",
    "cpu_minutes",
    "checks",
    "lane_runs",
];

/// The kind of a posted row: the export table it came from (`"table"` in the row), or `None`
/// for an export's header and for a findings report.
#[must_use]
pub fn kind_of(row: &serde_json::Value) -> Option<&str> {
    row.get("table").and_then(serde_json::Value::as_str)
}

/// Deletes up to [`DELETE_BATCHES`] batches of [`DELETE_BATCH`] rows that arrived more than
/// [`KEEP_DAYS`] days before `now`, each batch in its own transaction, and returns how many
/// rows it deleted. A writer waiting on the hub waits for one batch at most.
pub fn delete_expired(conn: &Connection, now: Timestamp) -> Result<usize> {
    let cutoff = now.plus_seconds(-KEEP_DAYS * 86_400.0).to_string();
    let mut deleted = 0;
    for _ in 0..DELETE_BATCHES {
        let batch = conn.execute(
            "DELETE FROM metrics WHERE (machine, source, data) IN (
               SELECT machine, source, data FROM metrics WHERE at < ?1 LIMIT ?2)",
            params![cutoff, DELETE_BATCH],
        )?;
        deleted += batch;
        if (batch as i64) < DELETE_BATCH {
            break;
        }
    }
    Ok(deleted)
}

/// The migration that added `kind`: deletes the rows of [`REBUILT_KINDS`] and gives every row
/// left its kind, before the next migration indexes the table. Deleting rows is not a schema
/// change, so an older build still reads the table.
pub(crate) fn prune_and_fill_kinds(tx: &Transaction<'_>) -> Result<()> {
    let rebuilt = REBUILT_KINDS
        .iter()
        .map(|kind| format!("'{kind}'"))
        .collect::<Vec<_>>()
        .join(", ");
    tx.execute(
        &format!("DELETE FROM metrics WHERE json_extract(data, '$.table') IN ({rebuilt})"),
        [],
    )?;
    tx.execute(
        "UPDATE metrics SET kind = json_extract(data, '$.table')
         WHERE json_extract(data, '$.table') IS NOT NULL",
        [],
    )?;
    Ok(())
}

/// Old rows share their arrival time with the rest of their import, so their original order
/// within that time is unavailable. Give ties a fixed order. The insert trigger also numbers
/// rows from a hub release that predates the feed, so a rollback keeps cursors valid.
pub(crate) fn number_existing_rows(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "UPDATE metrics AS m SET ingest_id = numbered.id
         FROM (
             SELECT machine, source, data,
                    row_number() OVER (ORDER BY at, machine, source, data) AS id
             FROM metrics
         ) AS numbered
         WHERE (m.machine, m.source, m.data) = (numbered.machine, numbered.source, numbered.data);

         INSERT INTO meta(key, value)
         SELECT 'metrics_ingest_id', coalesce(max(ingest_id), 0) FROM metrics;

         CREATE TRIGGER metrics_number_insert AFTER INSERT ON metrics BEGIN
             UPDATE meta SET value = value + 1 WHERE key = 'metrics_ingest_id';
             UPDATE metrics SET ingest_id = (SELECT value FROM meta WHERE key = 'metrics_ingest_id')
             WHERE (machine, source, data) = (new.machine, new.source, new.data);
         END;",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
