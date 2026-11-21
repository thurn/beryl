//! Machine history, with one timestamp index for expiry and one per machine for recent readings.

use ignite_core::ids::TicketId;
use ignite_core::protocol::machine_state::{MachineSnapshot, ReclaimableDisk, StartBlock};
use ignite_core::time::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

pub const RETENTION_SECONDS: f64 = 14.0 * 86_400.0;

pub fn record(conn: &Connection, machine: &str, snapshot: &MachineSnapshot) -> Result<()> {
    conn.execute(
        "INSERT INTO machine_snapshots (
            machine, at, free_disk_gib, reclaimable_bytes, reclaimable_measured_at,
            next_ticket, next_ticket_disk_gib, start_block, start_reason, cpu_percent,
            queued_cores, swap_used_mb, idle_workers, working_workers, stalled_workers,
            down_workers, ready_tickets
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT (machine, at) DO NOTHING",
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
        ],
    )?;
    Ok(())
}

pub fn latest(conn: &Connection, machine: &str) -> Result<Option<MachineSnapshot>> {
    let snapshot = conn
        .query_row(
            "SELECT at, free_disk_gib, reclaimable_bytes, reclaimable_measured_at, next_ticket,
                    next_ticket_disk_gib, start_block, start_reason, cpu_percent, queued_cores,
                    swap_used_mb, idle_workers, working_workers, stalled_workers, down_workers,
                    ready_tickets
             FROM machine_snapshots WHERE machine = ?1 ORDER BY at DESC LIMIT 1",
            [machine],
            |row| {
                let measured: Option<String> = row.get(3)?;
                let ticket: Option<String> = row.get(4)?;
                let block: Option<String> = row.get(6)?;
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
            },
        )
        .optional()?;
    Ok(snapshot)
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
        let mut snapshot = MachineSnapshot {
            at: cutoff.plus_seconds(-0.001),
            free_disk_gib: Some(123),
            reclaimable: Some(ReclaimableDisk {
                bytes: 321,
                measured_at: cutoff,
            }),
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
        };
        record(&conn, "m-test", &snapshot).unwrap();
        snapshot.at = cutoff;
        record(&conn, "m-test", &snapshot).unwrap();
        snapshot.at = now;
        record(&conn, "m-test", &snapshot).unwrap();
        record(&conn, "m-test", &snapshot).unwrap();
        assert_eq!(delete_expired(&conn, now).unwrap(), 1);
        assert_eq!(latest(&conn, "m-test").unwrap(), Some(snapshot));
        let oldest: String = conn
            .query_row("SELECT min(at) FROM machine_snapshots", [], |r| r.get(0))
            .unwrap();
        assert_eq!(oldest, cutoff.to_string());
    }
}
