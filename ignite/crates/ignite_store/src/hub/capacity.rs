//! Recent capacity reports from the machine snapshot history.

use std::collections::BTreeMap;

use ignite_core::capacity::CapacityReport;
use ignite_core::ids::MachineId;
use ignite_core::placement::REPORT_MAX_AGE_S;
use ignite_core::time::Timestamp;
use rusqlite::{Connection, OptionalExtension};

use crate::{Result, StoreError};

/// The newest report per machine, including reports exactly one freshness window old.
pub fn latest(conn: &Connection, now: Timestamp) -> Result<BTreeMap<MachineId, CapacityReport>> {
    let mut query = conn.prepare_cached(
        "SELECT machine, capacity FROM machine_snapshots
         WHERE (machine, at) IN (
             SELECT machine, max(at) FROM machine_snapshots
             WHERE at BETWEEN ?1 AND ?2 AND capacity IS NOT NULL GROUP BY machine
         )",
    )?;
    let mut rows = query.query([
        now.plus_seconds(-REPORT_MAX_AGE_S).to_string(),
        now.to_string(),
    ])?;
    let mut reports = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let machine: String = row.get(0)?;
        let id = machine.parse().map_err(|error| {
            StoreError::Invalid(format!("invalid machine ID in capacity report: {error}"))
        })?;
        let json: String = row.get(1)?;
        reports.insert(id, decode(&machine, &json)?);
    }
    Ok(reports)
}

/// The newest report at or before `now`, including older reports for display.
pub fn for_machine(
    conn: &Connection,
    machine: &str,
    now: Timestamp,
) -> Result<Option<(Timestamp, CapacityReport)>> {
    let mut query = conn.prepare_cached(
        "SELECT at, capacity FROM machine_snapshots
         WHERE machine = ?1 AND at <= ?2 AND capacity IS NOT NULL
         ORDER BY at DESC LIMIT 1",
    )?;
    let row = query
        .query_row([machine, &now.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    row.map(|(at, json)| {
        let at = Timestamp::parse(&at).map_err(|error| StoreError::Invalid(error.to_string()))?;
        Ok((at, decode(machine, &json)?))
    })
    .transpose()
}

fn decode(machine: &str, json: &str) -> Result<CapacityReport> {
    serde_json::from_str(json).map_err(|error| {
        StoreError::Invalid(format!("invalid capacity report from {machine}: {error}"))
    })
}
