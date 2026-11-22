//! The fleet: machines, their workers, reservations and hand-offs.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use ignite_core::config::Os;
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
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let machine = heartbeat.machine.as_str();
    let cli_apps = serde_json::to_string(&heartbeat.cli_apps)
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
    let seen = tx.execute(
        "UPDATE machines SET last_seen = ?2, version = ?3, cli_apps = ?4 WHERE id = ?1",
        params![machine, now.to_string(), heartbeat.version, cli_apps],
    )?;
    if seen == 0 {
        return Err(StoreError::Invalid(format!("unknown machine {machine}")));
    }
    if let Some(families) = &heartbeat.families {
        let families =
            serde_json::to_string(families).map_err(|e| StoreError::Invalid(e.to_string()))?;
        tx.execute(
            "UPDATE machines SET families = ?2 WHERE id = ?1",
            params![machine, families],
        )?;
    }
    // A worker that reports the state it was in keeps the time it entered it.
    let mut since: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    {
        let mut statement = tx.prepare(
            "SELECT session, state, state_since FROM workers
             WHERE machine = ?1 AND state_since IS NOT NULL",
        )?;
        let rows = statement.query_map([machine], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
        })?;
        for row in rows {
            let (session, state, entered) = row?;
            since.insert(session, (state, entered));
        }
    }
    tx.execute("DELETE FROM workers WHERE machine = ?1", [machine])?;
    for worker in &heartbeat.workers {
        let files =
            serde_json::to_string(&worker.files).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let entered = match since.get(worker.session.as_str()) {
            Some((state, entered)) if *state == worker.state => entered.clone(),
            _ => now.to_string(),
        };
        tx.execute(
            "INSERT OR REPLACE INTO workers
               (machine, session, app, role, state, ticket, files, command, command_since,
                command_running, state_since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                machine,
                worker.session.as_str(),
                worker.app,
                "worker",
                worker.state,
                worker.ticket.as_ref().map(ToString::to_string),
                files,
                worker.command,
                worker.command_since.map(|since| since.to_string()),
                worker.command_running,
                entered,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// When the worker running `session` entered the `idle` state, if it is idle now and the hub
/// has seen it enter that state.
pub fn idle_since(conn: &Connection, session: &str) -> Result<Option<String>> {
    let since = conn
        .query_row(
            "SELECT state_since FROM workers WHERE session = ?1 AND state = 'idle'
             ORDER BY state_since LIMIT 1",
            [session],
            |row| row.get::<_, Option<String>>(0),
        )
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(since)
}

/// Every machine, ordered by name.
pub fn machines(conn: &Connection) -> Result<Vec<rows::Machine>> {
    let mut statement = conn.prepare(
        "SELECT id, name, os, cores, memory_mb, families, version, last_seen, cli_apps
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
                cli_apps: row.get(8)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(machines)
}

/// Every worker of a machine, ordered by session.
pub fn workers(conn: &Connection, machine: &str) -> Result<Vec<rows::Worker>> {
    let mut statement = conn.prepare(
        "SELECT machine, session, app, role, state, ticket, files, command, command_since,
                command_running, state_since
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
                state_since: row.get(10)?,
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
        "INSERT INTO reservations (ticket, machine, session, expires_at, code, provider, master_blocker)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            reservation.ticket.as_str(),
            reservation.machine.as_str(),
            reservation.session.as_str(),
            reservation.expires_at.to_string(),
            reservation.code,
            reservation.provider,
            reservation.master_blocker,
        ],
    )?;
    super::placement_deferrals::clear(conn, &reservation.ticket)?;
    Ok(())
}

/// A new one-time code for a reservation: 8 random hexadecimal digits.
pub fn reservation_code() -> Result<String> {
    let mut bytes = [0_u8; 4];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| StoreError::Invalid(format!("no random numbers: {error}")))?;
    Ok(format!("{:08x}", u32::from_be_bytes(bytes)))
}

/// The machine, code and provider of the live reservation of `ticket`, if it has one. The caller runs
/// [`lapse`] first.
pub fn reservation_of(
    conn: &Connection,
    ticket: &str,
) -> Result<Option<(String, String, Option<String>)>> {
    Ok(conn
        .query_row(
            "SELECT machine, code, provider FROM reservations WHERE ticket = ?1",
            [ticket],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

/// The worker's live reservation from ordinary dispatch. The caller runs [`lapse`] first.
/// Retrying a landing retrieves its successor reservation separately.
pub fn reservation_for_worker(
    conn: &Connection,
    machine: &str,
    session: &str,
) -> Result<Option<Reservation>> {
    read_reservation(conn, machine, session, None)
}

/// Links a chain reservation to the landing that created it, in the landing's transaction.
pub fn link_landing(conn: &Connection, ticket: &str, landed: &str) -> Result<()> {
    conn.execute(
        "UPDATE reservations SET after_landing = ?2 WHERE ticket = ?1",
        params![ticket, landed],
    )?;
    Ok(())
}

/// The still-live successor reserved by this landing, if it has not been claimed or released.
pub fn reservation_for_landing(
    conn: &Connection,
    machine: &str,
    session: &str,
    landed: &str,
    now: Timestamp,
) -> Result<Option<Reservation>> {
    Ok(read_reservation(conn, machine, session, Some(landed))?
        .filter(|reservation| reservation.expires_at > now))
}

fn read_reservation(
    conn: &Connection,
    machine: &str,
    session: &str,
    after_landing: Option<&str>,
) -> Result<Option<Reservation>> {
    let row: Option<(String, String, String, Option<String>, bool)> = conn
        .query_row(
            "SELECT ticket, expires_at, code, provider, master_blocker FROM reservations
             WHERE machine = ?1 AND session = ?2 AND after_landing IS ?3
             ORDER BY expires_at LIMIT 1",
            params![machine, session, after_landing],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((ticket, expires_at, code, provider, master_blocker)) = row else {
        return Ok(None);
    };
    let invalid =
        |what: &str, error: String| StoreError::Invalid(format!("reservation {what}: {error}"));
    Ok(Some(Reservation {
        provider,
        master_blocker,
        ticket: ticket
            .parse()
            .map_err(|e: ignite_core::ids::IdError| invalid("ticket", e.to_string()))?,
        machine: machine
            .parse()
            .map_err(|e: ignite_core::ids::IdError| invalid("machine", e.to_string()))?,
        session: session
            .parse()
            .map_err(|e: ignite_core::ids::IdError| invalid("session", e.to_string()))?,
        expires_at: Timestamp::parse(&expires_at).map_err(|e| invalid("expiry", e.to_string()))?,
        code,
    }))
}

/// The `machine` value of the pause that holds every machine.
pub const ALL_MACHINES: &str = "*";

/// Pauses or resumes every hand-off. Individual machine states are fleet settings.
pub fn set_paused_everywhere(conn: &Connection, paused: bool, now: Timestamp) -> Result<()> {
    if paused {
        conn.execute(
            "INSERT INTO pauses (machine, at) VALUES (?1, ?2) ON CONFLICT(machine) DO NOTHING",
            params![ALL_MACHINES, now.to_string()],
        )?;
    } else {
        conn.execute("DELETE FROM pauses WHERE machine = ?1", [ALL_MACHINES])?;
    }
    Ok(())
}

/// Whether hand-offs to every machine are paused (`ignite pool pause --all`).
pub fn paused_everywhere(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pauses WHERE machine = ?1)",
        [ALL_MACHINES],
        |row| row.get(0),
    )?)
}

/// The machine's hostname, if it has registered.
pub fn name_of(conn: &Connection, machine: &str) -> Result<Option<String>> {
    Ok(name_and_os_of(conn, machine)?.map(|(name, _)| name))
}

/// The registered hostname and operating system of a machine.
pub fn name_and_os_of(conn: &Connection, machine: &str) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row(
            "SELECT name, os FROM machines WHERE id = ?1",
            [machine],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// The hostname and capabilities last reported by this machine.
pub fn dispatch_capabilities(
    conn: &Connection,
    machine: &str,
) -> Result<Option<(String, Os, Vec<String>)>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT name, os, families FROM machines WHERE id = ?1",
            [machine],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(name, os, families)| {
        let (os, families) = parse_dispatch_capabilities(&os, &families)?;
        Ok((name, os, families))
    })
    .transpose()
}

/// Reads the OS and family list stored for a registered machine.
pub fn parse_dispatch_capabilities(os: &str, families: &str) -> Result<(Os, Vec<String>)> {
    let os = match os {
        "macos" => Os::Macos,
        "windows" => Os::Windows,
        _ => return Err(StoreError::Invalid(format!("unknown machine OS {os}"))),
    };
    let families =
        serde_json::from_str(families).map_err(|error| StoreError::Invalid(error.to_string()))?;
    Ok((os, families))
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
            provider: None,
            master_blocker: false,
            ticket: ticket.parse().unwrap(),
            machine: "m-7f3a".parse().unwrap(),
            session: "local_1".parse().unwrap(),
            expires_at,
            code: "7f3a0001".into(),
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
            sample: None,
            capacity: None,
            machine: "m-7f3a".parse().unwrap(),
            version: "d97ed0e".into(),
            workers: vec![
                worker("local_a", "working", Some("Tq44s.1")),
                worker("local_b", "idle", None),
            ],
            cli_apps: vec!["codex".into()],
            families: None,
        };
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        assert_eq!(workers(&conn, "m-7f3a").unwrap().len(), 2);
        assert_eq!(machines(&conn).unwrap()[0].cli_apps, r#"["codex"]"#);
        clock.advance(30.0);
        beat.workers = vec![worker("local_b", "stalled", None)];
        beat.cli_apps.clear();
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        let now = workers(&conn, "m-7f3a").unwrap();
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].state, "stalled");
        assert_eq!(machines(&conn).unwrap()[0].cli_apps, "[]");
        assert_eq!(
            machines(&conn).unwrap()[0].last_seen,
            "2026-11-17T14:00:30.000Z"
        );
    }

    /// A machine whose `ignite.toml` gained a family reports it on its next heartbeat; a
    /// heartbeat with no list, from an older daemon, leaves the list the hub holds.
    #[test]
    fn a_heartbeat_with_families_replaces_the_registered_list() {
        let mut conn = hub::open_in_memory().unwrap();
        let clock = clock();
        register(&conn, &register_message(), clock.now()).unwrap();
        let mut beat = Heartbeat {
            sample: None,
            capacity: None,
            machine: "m-7f3a".parse().unwrap(),
            version: "d97ed0e".into(),
            workers: vec![],
            cli_apps: vec![],
            families: Some(vec!["unit".to_owned(), "platform".to_owned()]),
        };
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        let reported = r#"["unit","platform"]"#;
        assert_eq!(machines(&conn).unwrap()[0].families, reported);
        beat.families = None;
        heartbeat(&mut conn, &beat, clock.now()).unwrap();
        assert_eq!(machines(&conn).unwrap()[0].families, reported);
    }

    #[test]
    fn a_heartbeat_from_an_unregistered_machine_is_refused() {
        let mut conn = hub::open_in_memory().unwrap();
        let beat = Heartbeat {
            sample: None,
            capacity: None,
            machine: "m-unknown".parse().unwrap(),
            version: "x".into(),
            workers: vec![],
            cli_apps: vec![],
            families: None,
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
            sample: None,
            capacity: None,
            machine: "m-7f3a".parse().unwrap(),
            version: "d97ed0e".into(),
            workers: vec![running, worker("local_2", "idle", None)],
            cli_apps: vec![],
            families: None,
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
