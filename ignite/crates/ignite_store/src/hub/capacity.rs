//! Recent capacity reports from the machine snapshot history.

use std::collections::BTreeMap;

use ignite_core::capacity::CapacityReport;
use ignite_core::ids::MachineId;
use ignite_core::placement::REPORT_MAX_AGE_S;
use ignite_core::time::Timestamp;
use rusqlite::Connection;

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
        let report = serde_json::from_str(&json).map_err(|error| {
            StoreError::Invalid(format!("invalid capacity report from {machine}: {error}"))
        })?;
        reports.insert(id, report);
    }
    Ok(reports)
}
