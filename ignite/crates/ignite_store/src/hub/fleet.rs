//! The fleet: machines, their workers, reservations and hand-offs.

use rusqlite::{Connection, params};

use ignite_core::protocol::hub::{Heartbeat, MachineRegister, Reservation};
use ignite_core::time::Timestamp;

use crate::hub::rows;
use crate::{Result, StoreError};

/// How long a reservation waits for `ignite start` to claim it before it lapses.
pub const RESERVATION_SECONDS: f64 = 600.0;

/// When a reservation made at `now` lapses.
pub fn reservation_expiry(now: Timestamp) -> Timestamp {
    now.plus_seconds(RESERVATION_SECONDS)
}

/// Adds the machine, or replaces what the hub knows of it, and marks it seen at `now`.
pub fn register(conn: &Connection, machine: &MachineRegister, now: Timestamp) -> Result<()> {
    let families =
        serde_json::to_string(&machine.families).map_err(|e| StoreError::Invalid(e.to_string()))?;
    conn.execute(
        "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, os = excluded.os,
           cores = excluded.cores, memory_mb = excluded.memory_mb,
           families = excluded.families, version = excluded.version,
           last_seen = excluded.last_seen",
        params![
            machine.machine.as_str(),
            machine.name,
            machine.os.as_str(),
            i64::from(machine.cores),
            machine.memory_mb as i64,
            families,
            machine.version,
            now.to_string(),
        ],
    )?;
    Ok(())
}

/// Marks the machine seen and replaces its workers with the ones it reports, so a worker the
/// machine no longer lists disappears.
pub fn heartbeat(conn: &mut Connection, heartbeat: &Heartbeat, now: Timestamp) -> Result<()> {
    let tx = conn.transaction()?;
    let machine = heartbeat.machine.as_str();
    let seen = tx.execute(
        "UPDATE machines SET last_seen = ?2, version = ?3 WHERE id = ?1",
        params![machine, now.to_string(), heartbeat.version],
    )?;
    if seen == 0 {
        return Err(StoreError::Invalid(format!("unknown machine {machine}")));
    }
    tx.execute("DELETE FROM workers WHERE machine = ?1", [machine])?;
    for worker in &heartbeat.workers {
        let files =
            serde_json::to_string(&worker.files).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let role = match worker.role {
            ignite_core::protocol::hub::WorkerRole::Worker => "worker",
            ignite_core::protocol::hub::WorkerRole::Dispatcher => "dispatcher",
        };
        tx.execute(
            "INSERT OR REPLACE INTO workers
               (machine, session, app, role, state, ticket, files, command, command_since,
                command_running)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                machine,
                worker.session.as_str(),
                worker.app,
                role,
                worker.state,
                worker.ticket.as_ref().map(ToString::to_string),
                files,
                worker.command,
                worker.command_since.map(|since| since.to_string()),
                worker.command_running,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Every machine, ordered by name.
pub fn machines(conn: &Connection) -> Result<Vec<rows::Machine>> {
    let mut statement = conn.prepare(
        "SELECT id, name, os, cores, memory_mb, families, version, last_seen
         FROM machines ORDER BY name, id",
    )?;
    let machines = statement
        .query_map([], |row| {
            Ok(rows::Machine {
                id: row.get(0)?,
                name: row.get(1)?,
                os: row.get(2)?,
                cores: row.get(3)?,
                memory_mb: row.get(4)?,
                families: row.get(5)?,
                version: row.get(6)?,
                last_seen: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(machines)
}

/// Every worker of a machine, ordered by session.
pub fn workers(conn: &Connection, machine: &str) -> Result<Vec<rows::Worker>> {
    let mut statement = conn.prepare(
        "SELECT machine, session, app, role, state, ticket, files, command, command_since,
                command_running
         FROM workers WHERE machine = ?1 ORDER BY session",
    )?;
    let workers = statement
        .query_map([machine], |row| {
            Ok(rows::Worker {
                machine: row.get(0)?,
                session: row.get(1)?,
                app: row.get(2)?,
                role: row.get(3)?,
                state: row.get(4)?,
                ticket: row.get(5)?,
                files: row.get(6)?,
                command: row.get(7)?,
                command_since: row.get(8)?,
                command_running: row.get(9)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(workers)
}

/// Holds a ticket for a machine and session until the reservation's `expires_at`. Fails if the
/// ticket is already reserved, so two machines never take one ticket; the caller runs [`lapse`]
/// first so an expired reservation does not block it.
pub fn reserve(conn: &Connection, reservation: &Reservation) -> Result<()> {
    conn.execute(
        "INSERT INTO reservations (ticket, machine, session, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            reservation.ticket.as_str(),
            reservation.machine.as_str(),
            reservation.session.as_str(),
            reservation.expires_at.to_string(),
        ],
    )?;
    Ok(())
}

/// Turns a reservation into a claim by deleting it; returns whether it existed.
pub fn release_reservation(conn: &Connection, ticket: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM reservations WHERE ticket = ?1", [ticket])? > 0)
}

/// Deletes reservations that expired at or before `now`; returns their tickets, in order.
pub fn lapse(conn: &Connection, now: Timestamp) -> Result<Vec<String>> {
    let mut statement =
        conn.prepare("DELETE FROM reservations WHERE expires_at <= ?1 RETURNING ticket")?;
    let mut tickets = statement
        .query_map([now.to_string()], |row| row.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    tickets.sort();
    Ok(tickets)
}

/// Logs that `ticket` was handed to `machine`. `kind` is `regression`, `improvement` or
/// `product`.
pub fn record_handoff(
    conn: &Connection,
    machine: &str,
    ticket: &str,
    kind: &str,
    now: Timestamp,
) -> Result<()> {
    conn.execute(
        "INSERT INTO handoffs (machine, ticket, kind, at) VALUES (?1, ?2, ?3, ?4)",
        params![machine, ticket, kind, now.to_string()],
    )?;
    Ok(())
}

/// The last `n` hand-offs to a machine, newest first, not counting regression tickets: the
/// improvement share is a share of these.
pub fn recent_handoffs(conn: &Connection, machine: &str, n: u32) -> Result<Vec<rows::Handoff>> {
    let mut statement = conn.prepare(
        "SELECT seq, machine, ticket, kind, at FROM handoffs
         WHERE machine = ?1 AND kind != 'regression' ORDER BY seq DESC LIMIT ?2",
    )?;
    let handoffs = statement
        .query_map(params![machine, i64::from(n)], |row| {
            Ok(rows::Handoff {
                seq: row.get(0)?,
                machine: row.get(1)?,
                ticket: row.get(2)?,
                kind: row.get(3)?,
                at: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(handoffs)
}

#[cfg(test)]
mod tests {
    use ignite_core::config::Os;
    use ignite_core::fakes::FakeClock;
    use ignite_core::protocol::hub::{WorkerRole, WorkerState};
    use ignite_core::traits::Clock;

    use super::*;
    use crate::hub;

    fn clock() -> FakeClock {
        FakeClock::new("2026-11-17T14:00:00.000Z".parse().unwrap())
    }

    fn register_message() -> MachineRegister {
        MachineRegister {
            machine: "m-7f3a".parse().unwrap(),
            name: "jarvis".into(),
            os: Os::Macos,
            cores: 18,
            memory_mb: 65536,
            families: vec![],
            version: "d97ed0e".into(),
        }
    }

    fn worker(session: &str, state: &str, ticket: Option<&str>) -> WorkerState {
        WorkerState {
            session: session.parse().unwrap(),
            app: "claude".into(),
            role: WorkerRole::Worker,
            state: state.into(),
            ticket: ticket.map(|t| t.parse().unwrap()),
            files: vec!["rust/a.rs".into()],
            command: None,
            command_since: None,
            command_running: false,
        }
    }

    fn reservation(ticket: &str, expires_at: Timestamp) -> Reservation {
        Reservation {
            ticket: ticket.parse().unwrap(),
            machine: "m-7f3a".parse().unwrap(),
            session: "local_1".parse().unwrap(),
            expires_at,
        }
    }

    #[test]
    fn registering_twice_updates_one_machine() {
        let conn = hub::open_in_memory().unwrap();
        let clock = clock();
        register(&conn, &register_message(), clock.now()).unwrap();
        clock.advance(60.0);
        let mut update = register_message();
        update.version = "e1".into();
        register(&conn, &update, clock.now()).unwrap();
        let machines = machines(&conn).unwrap();
        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].version, "e1");
        assert_eq!(machines[0].last_seen, "2026-11-17T14:01:00.000Z");
    }

    #[test]
    fn a_heartbeat_replaces_the_workers_and_marks_the_machine_seen() {
        let mut conn = hub::open_in_memory().unwrap();
        let clock = clock();
        register(&conn, &register_message(), clock.now()).unwrap();
        let mut beat = Heartbeat {
            machine: "m-7f3a".parse().unwrap(),
            version: "d97ed0e".into(),
            workers: vec![
                worker("local_a", "working", Some("Tq44s.1")),
                worker("local_b", "idle", None),
            ],
        };
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        assert_eq!(workers(&conn, "m-7f3a").unwrap().len(), 2);
        clock.advance(30.0);
        beat.workers = vec![worker("local_b", "stalled", None)];
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        let now = workers(&conn, "m-7f3a").unwrap();
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].state, "stalled");
        assert_eq!(
            machines(&conn).unwrap()[0].last_seen,
            "2026-11-17T14:00:30.000Z"
        );
    }

    #[test]
    fn a_heartbeat_from_an_unregistered_machine_is_refused() {
        let mut conn = hub::open_in_memory().unwrap();
        let beat = Heartbeat {
            machine: "m-unknown".parse().unwrap(),
            version: "x".into(),
            workers: vec![],
        };
        assert!(heartbeat(&mut conn, &beat, clock().now()).is_err());
    }

    #[test]
    fn a_reservation_lapses_after_ten_minutes() {
        let conn = hub::open_in_memory().unwrap();
        let clock = clock();
        reserve(
            &conn,
            &reservation("Tq44s.1", reservation_expiry(clock.now())),
        )
        .unwrap();
        clock.advance(599.0);
        assert!(lapse(&conn, clock.now()).unwrap().is_empty());
        clock.advance(1.0);
        assert_eq!(lapse(&conn, clock.now()).unwrap(), vec!["Tq44s.1"]);
        assert!(lapse(&conn, clock.now()).unwrap().is_empty());
        // The ticket can be reserved again.
        reserve(
            &conn,
            &reservation("Tq44s.1", reservation_expiry(clock.now())),
        )
        .unwrap();
    }

    #[test]
    fn a_ticket_cannot_be_reserved_twice_and_a_claim_frees_it() {
        let conn = hub::open_in_memory().unwrap();
        let expiry = reservation_expiry(clock().now());
        reserve(&conn, &reservation("Tq44s.1", expiry)).unwrap();
        assert!(reserve(&conn, &reservation("Tq44s.1", expiry)).is_err());
        assert!(release_reservation(&conn, "Tq44s.1").unwrap());
        assert!(!release_reservation(&conn, "Tq44s.1").unwrap());
        reserve(&conn, &reservation("Tq44s.1", expiry)).unwrap();
    }

    #[test]
    fn the_last_handoffs_skip_regressions_and_other_machines() {
        let conn = hub::open_in_memory().unwrap();
        let now = clock().now();
        for i in 0..30 {
            let kind = match i % 3 {
                0 => "regression",
                1 => "improvement",
                _ => "product",
            };
            record_handoff(&conn, "m-7f3a", &format!("T{i}"), kind, now).unwrap();
            record_handoff(&conn, "m-other", &format!("O{i}"), "product", now).unwrap();
        }
        let recent = recent_handoffs(&conn, "m-7f3a", 20).unwrap();
        assert_eq!(recent.len(), 20);
        assert!(
            recent
                .iter()
                .all(|h| h.machine == "m-7f3a" && h.kind != "regression")
        );
        // Newest first: T29 is the latest non-regression (29 % 3 == 2).
        assert_eq!(recent[0].ticket, "T29");
        assert!(recent.windows(2).all(|pair| pair[0].seq > pair[1].seq));
        assert_eq!(recent_handoffs(&conn, "m-7f3a", 3).unwrap().len(), 3);
    }

    #[test]
    fn a_heartbeat_carries_the_command_each_worker_runs() {
        let mut conn = hub::open_in_memory().unwrap();
        let clock = clock();
        register(&conn, &register_message(), clock.now()).unwrap();
        let mut running = worker("local_1", "working", Some("q1.1"));
        running.command = Some("test".into());
        running.command_since = Some(clock.now());
        running.command_running = true;
        let beat = Heartbeat {
            machine: "m-7f3a".parse().unwrap(),
            version: "d97ed0e".into(),
            workers: vec![running, worker("local_2", "idle", None)],
        };
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        let stored = workers(&conn, "m-7f3a").unwrap();
        assert_eq!(stored[0].command.as_deref(), Some("test"));
        assert_eq!(
            stored[0].command_since.as_deref(),
            Some("2026-11-17T14:00:00.000Z")
        );
        assert!(stored[0].command_running);
        assert_eq!(stored[1].command, None);
        assert!(!stored[1].command_running);
    }
}
