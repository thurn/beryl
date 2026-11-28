//! The landings freeze that upkeep declares when `master` falls six hours behind.

use rusqlite::{Connection, params};

use ignite_core::fakes::FakeClock;
use ignite_core::ids::TicketId;
use ignite_core::protocol::emergency::{EMERGENCY_LASTS_S, Emergency, EmergencyScope};
use ignite_core::protocol::tickets::{Operation, Status, TicketType};
use ignite_core::traits::Clock;
use ignite_store::hub::{self, emergency};

use super::filing::Change;
use super::freeze::{self, FREEZE_SECONDS, FREEZE_SESSION};
use super::tests::{all_tickets, apply, comment_count, create, start, ticket};

/// Records a landing of a new ticket at `clock`'s time and returns the ticket.
fn landing(conn: &mut Connection, clock: &FakeClock) -> String {
    let id = apply(
        conn,
        vec![create("work", TicketType::Task, None, &[])],
        clock.now(),
    )
    .remove(0);
    conn.execute(
        "UPDATE tickets SET landed_at = ?1 WHERE id = ?2",
        params![clock.now().to_string(), id],
    )
    .unwrap();
    id
}

/// A new open ticket carrying `label`.
fn labelled(conn: &mut Connection, clock: &FakeClock, label: &str) -> String {
    let id = apply(
        conn,
        vec![create(label, TicketType::Bug, None, &[])],
        clock.now(),
    )
    .remove(0);
    let op = Operation::Update {
        id: id.clone(),
        set: None,
        add_labels: vec![label.to_owned()],
        remove_labels: Vec::new(),
        set_meta: std::collections::BTreeMap::default(),
        unset_meta: Vec::new(),
    };
    apply(conn, vec![op], clock.now());
    id
}

/// Records that tip verification moved `master` at `clock`'s time.
fn verify(conn: &Connection, clock: &FakeClock, commit: &str) {
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, verified, state) VALUES (?1, ?2, ?2, 'verified')",
        params![commit, clock.now().to_string()],
    )
    .unwrap();
}

fn in_force(conn: &Connection, clock: &FakeClock) -> Option<Emergency> {
    emergency::in_force(conn, clock.now()).unwrap()
}

fn id(ticket: &str) -> TicketId {
    TicketId::new(ticket).unwrap()
}

/// The tickets `ticket` depends on.
fn waits_on(conn: &Connection, ticket: &str) -> Vec<String> {
    conn.prepare("SELECT dst FROM edges WHERE src = ?1 ORDER BY dst")
        .unwrap()
        .query_map([ticket], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The open postmortem tickets, which upkeep files for every six-hour stall.
fn postmortems(conn: &Connection, clock: &FakeClock) -> Vec<String> {
    all_tickets(conn, clock.now())
        .into_iter()
        .filter(|t| t.status != Status::Closed && t.title.starts_with("Postmortem: "))
        .map(|t| t.id.to_string())
        .collect()
}

#[test]
fn six_hours_of_lag_freezes_landings_except_for_fixes() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let stuck = landing(&mut conn, &clock);
    let regression = labelled(&mut conn, &clock, "regression");
    clock.advance(FREEZE_SECONDS - 60.0);
    assert_eq!(freeze::pass(&mut conn, clock.now()).unwrap().filed, []);
    assert_eq!(in_force(&conn, &clock), None);

    clock.advance(120.0);
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    assert!(report.frozen);
    assert_eq!(report.filed.len(), 2);
    let frozen = in_force(&conn, &clock).expect("landings are frozen");
    assert_eq!(frozen.scope, EmergencyScope::Landings);
    assert_eq!(frozen.session.as_str(), FREEZE_SESSION);
    assert_eq!(frozen.ticket.as_str(), report.filed[0].ticket);

    let filed = ticket(&conn, &report.filed[0].ticket);
    assert!(filed.id.as_str().starts_with('J'), "{}", filed.id);
    assert_eq!(filed.priority, 0);
    assert_eq!(filed.status, Status::Open);
    assert!(filed.labels.contains(&"justiciar".to_owned()));
    assert!(filed.body.contains(&stuck), "{}", filed.body);

    // The postmortem waits on the freeze's ticket, and its fixes to Ignite may land.
    let postmortem = ticket(&conn, &report.filed[1].ticket);
    assert_eq!(postmortems(&conn, &clock), [postmortem.id.to_string()]);
    assert_eq!(postmortem.priority, 1);
    assert_eq!(postmortem.status, Status::Open);
    assert!(postmortem.labels.contains(&"justiciar".to_owned()));
    assert!(postmortem.body.contains(&stuck), "{}", postmortem.body);
    assert_eq!(
        waits_on(&conn, postmortem.id.as_str()),
        [filed.id.to_string()]
    );
    assert!(frozen.allows(&postmortem.id, "land"));

    // Daemons refuse `land` to every ticket the emergency does not allow.
    assert!(frozen.allows(&id(&regression), "land"));
    assert!(frozen.allows(&frozen.ticket, "land"));
    let ordinary = landing(&mut conn, &clock);
    assert!(!frozen.allows(&id(&ordinary), "land"));
}

#[test]
fn the_freeze_is_renewed_past_its_lapse_and_allows_new_fixes() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(FREEZE_SECONDS);
    freeze::pass(&mut conn, clock.now()).unwrap();
    let declared = in_force(&conn, &clock).unwrap();

    let incident = labelled(&mut conn, &clock, "incident");
    for _ in 0..3 {
        clock.advance(EMERGENCY_LASTS_S / 2.0);
        freeze::pass(&mut conn, clock.now()).unwrap();
    }
    let renewed = in_force(&conn, &clock).expect("still frozen after the first two hours");
    assert_eq!(renewed.declared_at, declared.declared_at);
    assert_eq!(renewed.renewed_at, clock.now());
    assert!(renewed.allows(&id(&incident), "land"));
}

#[test]
fn landings_resume_once_master_holds_every_landing_before_the_freeze() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(FREEZE_SECONDS);
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    let frozen = report.filed[0].ticket.clone();

    // A fix lands during the freeze, and `master` reaches a tip before it.
    clock.advance(600.0);
    landing(&mut conn, &clock);
    clock.advance(60.0);
    verify(&conn, &clock, "aaaaaaa");
    clock.advance(60.0);
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    assert!(report.lifted);
    assert_eq!(in_force(&conn, &clock), None);
    // No worker holds the ticket, so upkeep closes it.
    assert_eq!(ticket(&conn, &frozen).status, Status::Closed);
    assert_eq!(comment_count(&conn, &frozen), 1);
}

/// A landing under the freeze's ticket leaves it open until `master` catches up, and a worker
/// that holds it when `master` does keeps it, to close after recording its mitigations.
#[test]
fn the_freeze_ticket_stays_open_until_master_catches_up() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(FREEZE_SECONDS);
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    let frozen = report.filed[0].ticket.clone();
    assert!(freeze::holds_open(&conn, &frozen).unwrap());
    assert!(!freeze::holds_open(&conn, &report.filed[1].ticket).unwrap());

    conn.execute(
        "UPDATE tickets SET assignee = 'local_j', status = 'in_progress' WHERE id = ?1",
        [&frozen],
    )
    .unwrap();
    verify(&conn, &clock, "aaaaaaa");
    clock.advance(60.0);
    assert!(freeze::pass(&mut conn, clock.now()).unwrap().lifted);
    assert!(!freeze::holds_open(&conn, &frozen).unwrap());
    assert_ne!(ticket(&conn, &frozen).status, Status::Closed);
    assert_eq!(comment_count(&conn, &frozen), 1);
}

#[test]
fn a_freeze_a_session_ended_is_not_declared_again_for_the_same_stall() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(FREEZE_SECONDS);
    freeze::pass(&mut conn, clock.now()).unwrap();
    // `ignite pool resume --all` from the ticket's worker.
    emergency::clear(&conn).unwrap();
    clock.advance(3600.0);
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.filed, []);
    assert_eq!(in_force(&conn, &clock), None);

    // Once `master` catches up, a later stall freezes landings again.
    verify(&conn, &clock, "aaaaaaa");
    clock.advance(60.0);
    freeze::pass(&mut conn, clock.now()).unwrap();
    landing(&mut conn, &clock);
    clock.advance(FREEZE_SECONDS);
    freeze::pass(&mut conn, clock.now()).unwrap();
    assert!(in_force(&conn, &clock).is_some());
}

#[test]
fn another_emergency_in_force_is_left_alone() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let incident = labelled(&mut conn, &clock, "incident");
    let declared = Emergency {
        ticket: id(&incident),
        session: ignite_core::ids::SessionId::new("local_i").unwrap(),
        machine: ignite_core::ids::MachineId::new("m-1").unwrap(),
        declared_at: clock.now(),
        renewed_at: clock.now(),
        scope: EmergencyScope::All,
        allowed: Vec::new(),
    };
    clock.advance(FREEZE_SECONDS);
    let declared = Emergency {
        declared_at: clock.now(),
        renewed_at: clock.now(),
        ..declared
    };
    emergency::impose(&conn, &declared).unwrap();
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    assert!(!report.frozen);
    assert_eq!(in_force(&conn, &clock), Some(declared));
    assert_postmortem_without_freeze(&mut conn, &clock, &report.filed);
}

/// A stall that did not freeze landings still gets one postmortem, which later passes leave be.
fn assert_postmortem_without_freeze(
    conn: &mut Connection,
    clock: &FakeClock,
    filed: &[super::filing::Filed],
) {
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    let postmortem = ticket(conn, &filed[0].ticket);
    assert!(
        postmortem.title.starts_with("Postmortem: "),
        "{}",
        postmortem.title
    );
    assert!(
        postmortem.body.contains("did not freeze"),
        "{}",
        postmortem.body
    );
    assert_eq!(waits_on(conn, postmortem.id.as_str()), Vec::<String>::new());
    clock.advance(600.0);
    let again = freeze::pass(conn, clock.now()).unwrap();
    assert!(again.filed.iter().all(|f| f.change == Change::Unchanged));
    assert_eq!(postmortems(conn, clock), [postmortem.id.to_string()]);
}

#[test]
fn a_tip_waiting_on_a_silent_machine_does_not_freeze_landings() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
         VALUES ('m-2', 'cygnus', 'windows', 8, 16000, '[]', 'v1', ?1)",
        [clock.now().to_string()],
    )
    .unwrap();
    landing(&mut conn, &clock);
    clock.advance(60.0);
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, state) VALUES ('bbbbbbb', ?1, 'verifying')",
        [clock.now().to_string()],
    )
    .unwrap();
    clock.advance(FREEZE_SECONDS);
    let report = freeze::pass(&mut conn, clock.now()).unwrap();
    assert!(!report.frozen);
    assert_eq!(in_force(&conn, &clock), None);
    assert_postmortem_without_freeze(&mut conn, &clock, &report.filed);
}
