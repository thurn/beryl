//! The backlog's ratios behind the improvement share: the estimated weekly saving of every open
//! improvement ticket, and the fleet's whole use last week, in each unit
//! (docs/ignite.md, "Rules for the share").
//!
//! - Core-hours come from the records' CPU seconds, and agent-hours from the wall time of the
//!   commands agents ran.
//! - Dollars come from the efficiency rows machines post: each model window's dollars, split by
//!   token class so that every priced dollar is counted once. A unit with no use last week is
//!   left out of the fraction.

use rusqlite::{Connection, params};

use ignite_core::protocol::tickets::Saving;
use ignite_core::shares::{self, Amounts, Unit};
use ignite_core::time::Timestamp;

use super::WEEK_SECONDS;

/// The amounts the share is computed from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ratios {
    /// The open improvement tickets' estimated weekly saving.
    pub backlog: Amounts,
    /// The fleet's use over the last week.
    pub last_week: Amounts,
    /// The largest of `backlog / last_week` over the units with any use.
    pub fraction: f64,
}

pub fn ratios(conn: &Connection, now: Timestamp) -> rusqlite::Result<Ratios> {
    let backlog = backlog(conn)?;
    let last_week = last_week(conn, now)?;
    Ok(Ratios {
        backlog,
        last_week,
        fraction: shares::backlog_fraction(&backlog, &last_week),
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
    let dollars = conn.query_row(
        "SELECT COALESCE(SUM(json_extract(data, '$.row.dollars')), 0) FROM metrics
         WHERE json_extract(data, '$.table') = 'window_dollars'
           AND json_extract(data, '$.row.split') = 'class'
           AND json_extract(data, '$.row.start') >= ?1",
        params![since],
        |row| row.get::<_, f64>(0),
    )?;
    Ok(Amounts {
        dollars,
        agent_hours: agent_s / 3600.0,
        core_hours: cpu_s / 3600.0,
    })
}
