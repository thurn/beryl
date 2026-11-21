//! Each machine's latest `POST /next`: what it said it had free and what the hub answered. The
//! hub keeps no other record of a machine's idle workers, free ticket trees or cores, and the
//! dashboard's `available` page reads these to say why a ready ticket waits.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::hub::MachineFree;
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

/// A machine's latest ask.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub at: Timestamp,
    pub free: MachineFree,
    /// The reserved ticket's ID, or the reason nothing was offered.
    pub answer: String,
}

/// Replaces `free`'s machine's latest ask.
pub fn record(conn: &Connection, free: &MachineFree, answer: &str, now: Timestamp) -> Result<()> {
    let json = serde_json::to_string(free).map_err(|e| StoreError::Invalid(e.to_string()))?;
    conn.execute(
        "INSERT OR REPLACE INTO machine_asks (machine, at, free, answer) VALUES (?1, ?2, ?3, ?4)",
        params![free.machine.as_str(), now.to_string(), json, answer],
    )?;
    Ok(())
}

/// The latest ask of `machine`, if the hub has one it can read. An ask a newer build wrote in a
/// form this one cannot read counts as none.
pub fn latest(conn: &Connection, machine: &str) -> Result<Option<Ask>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT at, free, answer FROM machine_asks WHERE machine = ?1",
            [machine],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    Ok(row.and_then(|(at, free, answer)| {
        Some(Ask {
            at: at.parse().ok()?,
            free: serde_json::from_str(&free).ok()?,
            answer,
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::config::Os;
    use ignite_core::ids::MachineId;

    #[test]
    fn the_latest_ask_replaces_the_one_before() {
        let conn = crate::hub::open_in_memory().unwrap();
        let mut free = MachineFree {
            machine: MachineId::new("m1").unwrap(),
            os: Os::Macos,
            idle_workers: Vec::new(),
            free_ticket_trees: 2,
            free_cores: 3.0,
            admit_cores: None,
            pressure: None,
            families: Vec::new(),
            name: None,
            justiciar_only: false,
            master_only: false,
            explain: true,
            room: None,
        };
        let first = Timestamp::from_millis(1_000);
        record(&conn, &free, "TA", first).unwrap();
        free.free_ticket_trees = 0;
        let second = Timestamp::from_millis(2_000);
        record(&conn, &free, "no ticket tree is free", second).unwrap();
        let ask = latest(&conn, "m1").unwrap().unwrap();
        assert_eq!(ask.at, second);
        assert_eq!(ask.free.free_ticket_trees, 0);
        assert_eq!(ask.answer, "no ticket tree is free");
        assert_eq!(latest(&conn, "m2").unwrap(), None);
    }
}
