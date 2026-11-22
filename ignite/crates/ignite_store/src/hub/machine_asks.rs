//! Each machine's latest `POST /next`: what it offered and what the hub answered. Available
//! uses the offer to explain why ready work waits. Fleet shows the reason when nothing was offered.

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
    /// The dispatch reason, if the hub offered no ticket.
    pub nothing: Option<String>,
}

/// Replaces `free`'s machine's latest ask.
pub fn record(
    conn: &Connection,
    free: &MachineFree,
    answer: std::result::Result<&str, &str>,
    now: Timestamp,
) -> Result<()> {
    let text = answer.unwrap_or_else(|reason| reason);
    let json = serde_json::to_string(free).map_err(|e| StoreError::Invalid(e.to_string()))?;
    conn.execute(
        "INSERT INTO machine_asks (machine, at, free, answer, no_ticket_reason)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(machine) DO UPDATE SET at = excluded.at, free = excluded.free,
            answer = excluded.answer, no_ticket_reason = excluded.no_ticket_reason
         WHERE excluded.at >= machine_asks.at",
        params![
            free.machine.as_str(),
            now.to_string(),
            json,
            text,
            answer.err()
        ],
    )?;
    Ok(())
}

/// The latest ask of `machine`, if the hub has one it can read. An ask a newer build wrote in a
/// form this one cannot read counts as none.
pub fn latest(conn: &Connection, machine: &str) -> Result<Option<Ask>> {
    let row = conn
        .query_row(
            "SELECT at, free, answer, no_ticket_reason FROM machine_asks WHERE machine = ?1",
            [machine],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            },
        )
        .optional()?;
    Ok(row.and_then(|(at, free, answer, nothing)| {
        Some(Ask {
            at: at.parse().ok()?,
            free: serde_json::from_str(&free).ok()?,
            answer,
            nothing,
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
        record(&conn, &free, Ok("TA"), first).unwrap();
        free.free_ticket_trees = 0;
        let second = Timestamp::from_millis(2_000);
        record(&conn, &free, Err("no ticket tree is free"), second).unwrap();
        let ask = latest(&conn, "m1").unwrap().unwrap();
        assert_eq!(ask.at, second);
        assert_eq!(ask.free.free_ticket_trees, 0);
        assert_eq!(ask.answer, "no ticket tree is free");
        assert_eq!(ask.nothing.as_deref(), Some("no ticket tree is free"));
        record(&conn, &free, Ok("TA"), first).unwrap();
        assert_eq!(latest(&conn, "m1").unwrap(), Some(ask));
        record(&conn, &free, Ok("TB"), second.plus_seconds(1.0)).unwrap();
        assert_eq!(latest(&conn, "m1").unwrap().unwrap().nothing, None);
        assert_eq!(latest(&conn, "m2").unwrap(), None);
    }
}
