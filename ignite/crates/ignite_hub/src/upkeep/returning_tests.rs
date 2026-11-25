use ignite_core::config::Os;
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::hub::{Heartbeat, MachineRegister, WorkerRole, WorkerState};
use ignite_core::protocol::returning::{RELEASED_OFFLINE_KEY, Reconciled, ReturnOutcome};
use ignite_core::protocol::tickets::{Operation, Status, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, fleet, tickets};
use rusqlite::Connection;

use super::reconcile;
use crate::upkeep::offline::release_claims;
use crate::upkeep::tests::{apply, create};

const SESSION: &str = "cli-claude-0a1b2c3d";

fn at(seconds: f64) -> Timestamp {
    Timestamp::parse("2026-11-25T06:00:00Z")
        .unwrap()
        .plus_seconds(seconds)
}

fn register(conn: &Connection, now: Timestamp) {
    let register = MachineRegister {
        machine: MachineId::new("m-1").unwrap(),
        name: "cygnus".to_owned(),
        os: Os::Windows,
        cores: 16,
        memory_mb: 65_536,
        families: Vec::new(),
        version: "v".to_owned(),
    };
    fleet::register(conn, &register, now).unwrap();
}

/// The heartbeat of a machine whose one CLI worker, `SESSION`, holds a tree for `ticket`.
fn beat(ticket: &str) -> Heartbeat {
    Heartbeat {
        tree_volumes: None,
        sample: None,
        capacity: None,
        machine: MachineId::new("m-1").unwrap(),
        version: "v".to_owned(),
        workers: vec![WorkerState {
            session: SessionId::new(SESSION).unwrap(),
            app: "claude".to_owned(),
            role: WorkerRole::Worker,
            state: "working".to_owned(),
            ticket: Some(TicketId::new(ticket).unwrap()),
            files: Vec::new(),
            command: None,
            command_since: None,
            command_running: false,
        }],
        cli_apps: vec!["claude".to_owned()],
        families: None,
    }
}

/// A ticket that `SESSION` on m-1 held in tree-07 until the hub released it for silence, at
/// 2 hours.
fn released_ticket(conn: &mut Connection) -> String {
    register(conn, at(0.0));
    let ids = apply(
        conn,
        vec![create("work", TicketType::Task, None, &[])],
        at(0.0),
    );
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = ?2, session = ?2,
           tree = 'tree-07', machine = 'm-1', provider = 'claude' WHERE id = ?1",
        rusqlite::params![ids[0], SESSION],
    )
    .unwrap();
    assert_eq!(release_claims(conn, at(7_200.0)).unwrap(), [ids[0].clone()]);
    ids[0].clone()
}

fn answer(id: &str, outcome: ReturnOutcome) -> Vec<Reconciled> {
    vec![Reconciled {
        session: SessionId::new(SESSION).unwrap(),
        ticket: TicketId::new(id).unwrap(),
        outcome,
    }]
}

#[test]
fn a_returning_worker_gets_back_the_ticket_nobody_took_for_the_same_session_and_tree() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = released_ticket(&mut conn);
    let released = tickets::load(&conn, &id).unwrap().unwrap();
    assert!(released.metadata.contains_key(RELEASED_OFFLINE_KEY));

    let answers = reconcile(&mut conn, &beat(&id), at(28_800.0)).unwrap();

    assert_eq!(answers, answer(&id, ReturnOutcome::Reclaimed));
    let ticket = tickets::load(&conn, &id).unwrap().unwrap();
    assert_eq!(ticket.status, Status::InProgress);
    assert_eq!(ticket.assignee.as_deref(), Some(SESSION));
    assert_eq!(
        ticket.session.as_ref().map(SessionId::as_str),
        Some(SESSION)
    );
    assert_eq!(
        ticket.tree.as_ref().map(ignite_core::ids::TreeId::as_str),
        Some("tree-07")
    );
    assert_eq!(ticket.machine.as_ref().map(MachineId::as_str), Some("m-1"));
    assert!(!ticket.metadata.contains_key(RELEASED_OFFLINE_KEY));
    let comments = tickets::comments(&conn, &id).unwrap();
    let last = &comments.last().unwrap().text;
    assert!(
        last.contains("Claimed again") && last.contains("cygnus"),
        "{last}"
    );
    // The next heartbeat finds the session holding it and says nothing.
    assert!(
        reconcile(&mut conn, &beat(&id), at(28_830.0))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_ticket_another_session_took_or_a_person_changed_is_lost_to_the_returning_worker() {
    let mut conn = hub::open_in_memory().unwrap();
    let taken = released_ticket(&mut conn);
    // Another session claims it while the machine is away.
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = 'cli-codex-99999999',
           session = 'cli-codex-99999999', updated_at = ?2 WHERE id = ?1",
        rusqlite::params![taken, at(9_000.0).to_string()],
    )
    .unwrap();
    assert_eq!(
        reconcile(&mut conn, &beat(&taken), at(28_800.0)).unwrap(),
        answer(&taken, ReturnOutcome::Lost)
    );
    let ticket = tickets::load(&conn, &taken).unwrap().unwrap();
    assert_eq!(ticket.assignee.as_deref(), Some("cli-codex-99999999"));

    // A person edits the open, unassigned ticket after the release: it stays open for whoever
    // the queue gives it to.
    let mut conn = hub::open_in_memory().unwrap();
    let changed = released_ticket(&mut conn);
    apply(
        &mut conn,
        vec![Operation::Update {
            id: changed.clone(),
            set: Some(TicketFields {
                priority: Some(0),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: std::collections::BTreeMap::new(),
            unset_meta: Vec::new(),
        }],
        at(9_000.0),
    );
    assert_eq!(
        reconcile(&mut conn, &beat(&changed), at(28_800.0)).unwrap(),
        answer(&changed, ReturnOutcome::Lost)
    );
    let ticket = tickets::load(&conn, &changed).unwrap().unwrap();
    assert_eq!(ticket.status, Status::Open);
    assert_eq!(ticket.assignee, None);
}

#[test]
fn a_ticket_the_hub_did_not_release_for_silence_is_left_alone() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&conn, at(0.0));
    let ids = apply(
        &mut conn,
        vec![create("work", TicketType::Task, None, &[])],
        at(0.0),
    );
    assert!(
        reconcile(&mut conn, &beat(&ids[0]), at(60.0))
            .unwrap()
            .is_empty()
    );
}
