//! Samples of the Claude plan's usage, one row per window (see `schema_19.sql`). The daemon
//! writes them when its dispatcher reports what the desktop app's `get_usage` returned, sends
//! the rows the hub has not acknowledged, and `ignite pool usage` reads the latest of each
//! window.

use rusqlite::{Connection, Row, TransactionBehavior, params};

use ignite_core::time::Timestamp;

use crate::Result;
use crate::local::rows::PlanUsage;

/// Rows older than this many days are deleted.
pub const KEEP_DAYS: f64 = 30.0;

const COLUMNS: &str = "id, at, machine, status, label, percent_used, resets_at, note, posted";

fn read(row: &Row<'_>) -> rusqlite::Result<PlanUsage> {
    Ok(PlanUsage {
        id: row.get(0)?,
        at: row.get(1)?,
        machine: row.get(2)?,
        status: row.get(3)?,
        label: row.get(4)?,
        percent_used: row.get(5)?,
        resets_at: row.get(6)?,
        note: row.get(7)?,
        posted: row.get(8)?,
    })
}

/// Stores the rows of one sample (their `id` and `posted` are ignored) in one transaction, and
/// deletes every row more than [`KEEP_DAYS`] days older than `now`.
pub fn insert(conn: &mut Connection, sample: &[PlanUsage], now: Timestamp) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    {
        let mut insert = transaction.prepare(
            "INSERT INTO plan_usage (at, machine, status, label, percent_used, resets_at, note) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for row in sample {
            insert.execute(params![
                row.at,
                row.machine,
                row.status,
                row.label,
                row.percent_used,
                row.resets_at,
                row.note
            ])?;
        }
    }
    let cutoff = now.plus_seconds(-KEEP_DAYS * 86_400.0);
    transaction.execute(
        "DELETE FROM plan_usage WHERE at < ?1",
        params![cutoff.to_string()],
    )?;
    transaction.commit()?;
    Ok(())
}

/// The rows not yet acknowledged by the hub, oldest first.
pub fn unposted(conn: &Connection) -> Result<Vec<PlanUsage>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM plan_usage WHERE posted = 0 ORDER BY id"
    ))?;
    let found = statement
        .query_map([], read)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(found)
}

/// Marks the rows `ids` as acknowledged by the hub.
pub fn mark_posted(conn: &mut Connection, ids: &[i64]) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    {
        let mut update = transaction.prepare("UPDATE plan_usage SET posted = 1 WHERE id = ?1")?;
        for id in ids {
            update.execute(params![id])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// The newest row of each window, ordered by label, followed by the newest sample that had no
/// windows if it is newer than every window's row.
pub fn latest(conn: &Connection) -> Result<Vec<PlanUsage>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM plan_usage p WHERE label IS NOT NULL AND id = \
         (SELECT MAX(id) FROM plan_usage q WHERE q.label = p.label) ORDER BY label"
    ))?;
    let mut found: Vec<PlanUsage> = statement
        .query_map([], read)?
        .collect::<rusqlite::Result<_>>()?;
    let newest_window = found.iter().map(|row| row.id).max().unwrap_or(0);
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM plan_usage WHERE label IS NULL AND id > ?1 \
         ORDER BY id DESC LIMIT 1"
    ))?;
    found.extend(
        statement
            .query_map(params![newest_window], read)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    Ok(found)
}

/// When the newest sample was taken, if there is one.
pub fn newest_at(conn: &Connection) -> Result<Option<String>> {
    let mut statement = conn.prepare("SELECT MAX(at) FROM plan_usage")?;
    Ok(statement.query_row([], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn row(at: &str, label: Option<&str>, percent: Option<f64>) -> PlanUsage {
        PlanUsage {
            id: 0,
            at: at.to_owned(),
            machine: "m-1".to_owned(),
            status: if label.is_some() { "ok" } else { "unavailable" }.to_owned(),
            label: label.map(str::to_owned),
            percent_used: percent,
            resets_at: None,
            note: None,
            posted: 0,
        }
    }

    #[test]
    fn the_latest_row_of_each_window_is_read_and_posting_is_remembered() {
        let mut conn = open_in_memory().unwrap();
        let now: Timestamp = "2026-11-20T12:00:00.000Z".parse().unwrap();
        let first = "2026-11-20T10:00:00.000Z";
        let second = "2026-11-20T11:00:00.000Z";
        let old = "2026-10-01T00:00:00.000Z";
        insert(&mut conn, &[row(old, Some("Weekly"), Some(1.0))], now).unwrap();
        assert!(
            latest(&conn).unwrap().is_empty(),
            "a row past 30 days is deleted"
        );
        insert(
            &mut conn,
            &[
                row(first, Some("Weekly"), Some(10.0)),
                row(first, Some("5h"), Some(50.0)),
            ],
            now,
        )
        .unwrap();
        insert(&mut conn, &[row(second, Some("Weekly"), Some(12.0))], now).unwrap();
        let rows = latest(&conn).unwrap();
        let summary: Vec<(Option<&str>, Option<f64>)> = rows
            .iter()
            .map(|r| (r.label.as_deref(), r.percent_used))
            .collect();
        assert_eq!(
            summary,
            [(Some("5h"), Some(50.0)), (Some("Weekly"), Some(12.0))]
        );
        insert(
            &mut conn,
            &[row("2026-11-20T11:30:00.000Z", None, None)],
            now,
        )
        .unwrap();
        let rows = latest(&conn).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].status, "unavailable");
        assert_eq!(
            newest_at(&conn).unwrap().as_deref(),
            Some("2026-11-20T11:30:00.000Z")
        );

        let waiting = unposted(&conn).unwrap();
        assert_eq!(waiting.len(), 4);
        mark_posted(&mut conn, &[waiting[0].id, waiting[1].id]).unwrap();
        assert_eq!(unposted(&conn).unwrap().len(), 2);
    }
}
