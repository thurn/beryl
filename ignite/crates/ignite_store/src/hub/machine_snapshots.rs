//! Machine history, with one timestamp index for expiry and one per machine for recent readings.

use ignite_core::capacity::CapacityReport;
use ignite_core::ids::TicketId;
use ignite_core::protocol::machine_state::{MachineSnapshot, ReclaimableDisk, StartBlock};
use ignite_core::time::Timestamp;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Result, StoreError};

pub const RETENTION_SECONDS: f64 = 14.0 * 86_400.0;

pub fn record(
    conn: &Connection,
    machine: &str,
    snapshot: &MachineSnapshot,
    capacity: Option<&CapacityReport>,
) -> Result<()> {
    let capacity = capacity
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StoreError::Invalid(format!("cannot encode machine capacity: {error}")))?;
    conn.execute(
        "INSERT INTO machine_snapshots (
            machine, at, free_disk_gib, reclaimable_bytes, reclaimable_measured_at,
            next_ticket, next_ticket_disk_gib, start_block, start_reason, cpu_percent,
            queued_cores, swap_used_mb, idle_workers, working_workers, stalled_workers,
            down_workers, ready_tickets, capacity, last_disk_cleanup_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT (machine, at) DO UPDATE SET capacity = excluded.capacity
         WHERE machine_snapshots.capacity IS NULL AND excluded.capacity IS NOT NULL",
        params![
            machine,
            snapshot.at.to_string(),
            snapshot.free_disk_gib.map(signed).transpose()?,
            snapshot
                .reclaimable
                .map(|disk| signed(disk.bytes))
                .transpose()?,
            snapshot
                .reclaimable
                .map(|disk| disk.measured_at.to_string()),
            snapshot.next_ticket.as_ref().map(TicketId::as_str),
            snapshot.next_ticket_disk_gib.map(signed).transpose()?,
            snapshot.start_block.map(StartBlock::as_str),
            snapshot.start_reason,
            snapshot.cpu_percent,
            signed(snapshot.queued_cores)?,
            snapshot.swap_used_mb.map(signed).transpose()?,
            snapshot.idle_workers,
            snapshot.working_workers,
            snapshot.stalled_workers,
            snapshot.down_workers,
            snapshot.ready_tickets,
            capacity,
            snapshot.last_disk_cleanup_at.map(|at| at.to_string()),
        ],
    )?;
    Ok(())
}

const SELECT: &str =
    "SELECT at, free_disk_gib, reclaimable_bytes, reclaimable_measured_at, next_ticket,
            next_ticket_disk_gib, start_block, start_reason, cpu_percent, queued_cores,
            swap_used_mb, idle_workers, working_workers, stalled_workers, down_workers,
            ready_tickets, last_disk_cleanup_at
     FROM machine_snapshots";

pub fn latest(conn: &Connection, machine: &str) -> Result<Option<MachineSnapshot>> {
    Ok(conn
        .query_row(
            &format!("{SELECT} WHERE machine = ?1 ORDER BY at DESC LIMIT 1"),
            [machine],
            read,
        )
        .optional()?)
}

/// Samples for one machine in the inclusive time range, oldest first.
pub fn between(
    conn: &Connection,
    machine: &str,
    from: Timestamp,
    through: Timestamp,
) -> Result<Vec<MachineSnapshot>> {
    let mut statement = conn.prepare_cached(&format!(
        "{SELECT} WHERE machine = ?1 AND at BETWEEN ?2 AND ?3 ORDER BY at"
    ))?;
    let snapshots = statement
        .query_map(
            params![machine, from.to_string(), through.to_string()],
            read,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(snapshots)
}

fn read(row: &Row<'_>) -> rusqlite::Result<MachineSnapshot> {
    let measured: Option<String> = row.get(3)?;
    let ticket: Option<String> = row.get(4)?;
    let block: Option<String> = row.get(6)?;
    let cleanup: Option<String> = row.get(16)?;
    Ok(MachineSnapshot {
        at: parse(&row.get::<_, String>(0)?, 0)?,
        free_disk_gib: row
            .get::<_, Option<i64>>(1)?
            .map(|value| unsigned(value, 1))
            .transpose()?,
        reclaimable: measured
            .map(|at| {
                Ok::<_, rusqlite::Error>(ReclaimableDisk {
                    bytes: unsigned(row.get(2)?, 2)?,
                    measured_at: parse(&at, 3)?,
                })
            })
            .transpose()?,
        last_disk_cleanup_at: cleanup.map(|at| parse(&at, 16)).transpose()?,
        next_ticket: ticket.map(|id| parse(&id, 4)).transpose()?,
        next_ticket_disk_gib: row
            .get::<_, Option<i64>>(5)?
            .map(|value| unsigned(value, 5))
            .transpose()?,
        start_block: block
            .map(|block| serde_json::from_value(serde_json::Value::String(block)))
            .transpose()
            .map_err(|error| conversion(6, error))?,
        start_reason: row.get(7)?,
        cpu_percent: row.get(8)?,
        queued_cores: unsigned(row.get(9)?, 9)?,
        swap_used_mb: row
            .get::<_, Option<i64>>(10)?
            .map(|value| unsigned(value, 10))
            .transpose()?,
        idle_workers: row.get(11)?,
        working_workers: row.get(12)?,
        stalled_workers: row.get(13)?,
        down_workers: row.get(14)?,
        ready_tickets: row.get(15)?,
    })
}

fn signed(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn unsigned(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn parse<T: std::str::FromStr>(text: &str, column: usize) -> rusqlite::Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.parse().map_err(|error| conversion(column, error))
}

fn conversion(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(error))
}

pub fn delete_expired(conn: &Connection, now: Timestamp) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM machine_snapshots WHERE at < ?1",
        [now.plus_seconds(-RETENTION_SECONDS).to_string()],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(at: Timestamp) -> MachineSnapshot {
        MachineSnapshot {
            at,
            free_disk_gib: Some(123),
            reclaimable: Some(ReclaimableDisk {
                bytes: 321,
                measured_at: at,
            }),
            last_disk_cleanup_at: Some(at),
            next_ticket: Some("Tabcd".parse().unwrap()),
            next_ticket_disk_gib: Some(140),
            start_block: Some(StartBlock::Disk),
            start_reason: Some("Needs more disk".into()),
            cpu_percent: Some(76),
            queued_cores: 22,
            swap_used_mb: Some(100),
            idle_workers: 4,
            working_workers: 2,
            stalled_workers: 1,
            down_workers: 1,
            ready_tickets: 4,
        }
    }

    #[test]
    fn machine_snapshot_retention_keeps_the_boundary_and_newer_samples() {
        let conn = crate::hub::open_in_memory().unwrap();
        let now = Timestamp::EPOCH.plus_seconds(RETENTION_SECONDS + 60.0);
        let cutoff = now.plus_seconds(-RETENTION_SECONDS);
        conn.execute(
            "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
             VALUES ('m-test', 'test', 'macos', 8, 16000, '[]', 'v1', ?1)",
            [now.to_string()],
        )
        .unwrap();
        let mut snapshot = fixture(cutoff.plus_seconds(-0.001));
        record(&conn, "m-test", &snapshot, None).unwrap();
        snapshot.at = cutoff;
        record(&conn, "m-test", &snapshot, None).unwrap();
        snapshot.at = now;
        record(&conn, "m-test", &snapshot, None).unwrap();
        record(&conn, "m-test", &snapshot, None).unwrap();
        assert_eq!(delete_expired(&conn, now).unwrap(), 1);
        assert_eq!(latest(&conn, "m-test").unwrap(), Some(snapshot));
        let oldest: String = conn
            .query_row("SELECT min(at) FROM machine_snapshots", [], |r| r.get(0))
            .unwrap();
        assert_eq!(oldest, cutoff.to_string());
    }

    #[test]
    fn latest_capacity_keeps_one_fresh_report_per_machine() {
        let conn = crate::hub::open_in_memory().unwrap();
        let now = Timestamp::EPOCH.plus_seconds(120.0);
        for machine in ["m-current", "m-boundary", "m-old", "m-legacy"] {
            conn.execute(
                "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
                 VALUES (?1, ?1, 'macos', 8, 16000, '[]', 'v1', ?2)",
                params![machine, now.to_string()],
            )
            .unwrap();
        }
        for (machine, age, cores) in [
            ("m-current", 10.0, Some(8)),
            ("m-current", 20.0, Some(6)),
            ("m-current", 0.0, None),
            ("m-boundary", 60.0, Some(4)),
            ("m-old", 60.001, Some(2)),
            ("m-legacy", 0.0, None),
        ] {
            let snapshot = fixture(now.plus_seconds(-age));
            let report = cores.map(|logical_cores| CapacityReport {
                logical_cores,
                ..CapacityReport::default()
            });
            record(&conn, machine, &snapshot, report.as_ref()).unwrap();
        }

        let reports = crate::hub::capacity::latest(&conn, now).unwrap();
        let found: Vec<_> = reports
            .iter()
            .map(|(id, report)| (id.as_str(), report.logical_cores))
            .collect();
        assert_eq!(found, [("m-boundary", 4), ("m-current", 8)]);
        assert!(latest(&conn, "m-old").unwrap().is_some());
    }
}
