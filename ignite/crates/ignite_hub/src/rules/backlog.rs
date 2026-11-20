//! The backlog's ratios behind the improvement share: the estimated weekly saving of every open
//! improvement ticket, and the fleet's whole use last week, in each unit
//! (docs/ignite.md, "Rules for the share").
//!
//! - Core-hours come from the records' CPU seconds, and agent-hours from the wall time of the
//!   commands agents ran.
//! - Dollars come from the efficiency rows machines post: each model window's dollars, split by
//!   token class so that every priced dollar is counted once. A unit with no use last week is
//!   left out of the fraction.
//! - Last week's use is read from the `records` and `metrics` tables, which hold gigabytes, so
//!   it is taken once per rule evaluation ([`refresh`], called from
//!   `filing::evaluate_if_due`) and kept in `meta`. [`ratios`] runs inside the dispatch
//!   transaction, which holds the database's write lock, and reads only that one row and the
//!   open tickets' savings. When last week's use was read inside that transaction, a slow
//!   `metrics` scan held the write lock for minutes and every other hub write failed with
//!   `database is locked`.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use ignite_core::protocol::tickets::Saving;
use ignite_core::shares::{self, Amounts, Unit};
use ignite_core::time::Timestamp;

use super::WEEK_SECONDS;

/// Where last week's use is kept in `meta`, as `{"dollars", "agent_hours", "core_hours"}`.
const LAST_WEEK_KEY: &str = "backlog_last_week";

/// The amounts the share is computed from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ratios {
    /// The open improvement tickets' estimated weekly saving.
    pub backlog: Amounts,
    /// The fleet's use over the week before the last [`refresh`], or nothing before the first.
    pub last_week: Amounts,
    /// The largest of `backlog / last_week` over the units with any use.
    pub fraction: f64,
}

/// The ratios from the open tickets and the last week's use that [`refresh`] stored. Reads no
/// `records` or `metrics` rows, so it is safe inside a write transaction.
pub fn ratios(conn: &Connection) -> rusqlite::Result<Ratios> {
    let backlog = backlog(conn)?;
    let last_week = stored_last_week(conn)?;
    Ok(Ratios {
        backlog,
        last_week,
        fraction: shares::backlog_fraction(&backlog, &last_week),
    })
}

/// Takes the fleet's use over the week before `now` and stores it for [`ratios`].
pub fn refresh(conn: &Connection, now: Timestamp) -> rusqlite::Result<()> {
    let amounts = last_week(conn, now)?;
    let value = json!({
        "dollars": amounts.dollars,
        "agent_hours": amounts.agent_hours,
        "core_hours": amounts.core_hours,
    });
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params![LAST_WEEK_KEY, value.to_string()],
    )?;
    Ok(())
}

/// The use [`refresh`] stored last; nothing when it never ran or its row does not parse.
fn stored_last_week(conn: &Connection) -> rusqlite::Result<Amounts> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [LAST_WEEK_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let Some(value) = text.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Ok(Amounts::default());
    };
    let amount = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0)
    };
    Ok(Amounts {
        dollars: amount("dollars"),
        agent_hours: amount("agent_hours"),
        core_hours: amount("core_hours"),
    })
}

/// The estimated weekly saving of every open improvement ticket, by unit.
fn backlog(conn: &Connection) -> rusqlite::Result<Amounts> {
    let mut statement =
        conn.prepare("SELECT saving FROM tickets WHERE status != 'closed' AND saving IS NOT NULL")?;
    let savings = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut amounts = Amounts::default();
    for text in savings {
        let Ok(saving) = serde_json::from_str::<Saving>(&text) else {
            continue;
        };
        match Unit::parse(&saving.unit) {
            Some(Unit::Dollars) => amounts.dollars += saving.amount,
            Some(Unit::AgentHours) => amounts.agent_hours += saving.amount,
            Some(Unit::CoreHours) => amounts.core_hours += saving.amount,
            None => {}
        }
    }
    Ok(amounts)
}

/// The dollars of the class-split model windows that started at or after `?1`. A window is
/// posted after it starts, so its row arrived (`at`) no earlier than its `start`: the
/// `(kind, at)` index narrows the rows to that span before any JSON is read.
pub(crate) const WINDOW_DOLLARS: &str =
    "SELECT COALESCE(SUM(json_extract(data, '$.row.dollars')), 0) FROM metrics
     WHERE kind = 'window_dollars' AND at >= ?1
       AND json_extract(data, '$.row.split') = 'class'
       AND json_extract(data, '$.row.start') >= ?1";

/// The fleet's use over the week before `now`.
fn last_week(conn: &Connection, now: Timestamp) -> rusqlite::Result<Amounts> {
    let since = now.plus_seconds(-WEEK_SECONDS).to_string();
    let (cpu_s, agent_s) = conn.query_row(
        "SELECT COALESCE(SUM(cpu_s), 0),
           COALESCE(SUM(CASE WHEN class = 'agent' THEN wall_s ELSE 0 END), 0)
         FROM records WHERE at >= ?1",
        params![since],
        |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
    )?;
    let dollars = conn.query_row(WINDOW_DOLLARS, params![since], |row| row.get::<_, f64>(0))?;
    Ok(Amounts {
        dollars,
        agent_hours: agent_s / 3600.0,
        core_hours: cpu_s / 3600.0,
    })
}
