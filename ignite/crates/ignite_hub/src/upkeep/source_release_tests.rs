//! Opening the hub's pending tickets once their source has workers ([`super::source_release`]).

use std::collections::BTreeMap;

use rusqlite::Connection;

use ignite_core::fleet::Allocations;
use ignite_core::protocol::tickets::{BatchRequest, Operation, Status, TicketFields};
use ignite_core::shares::Source;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{self, fleet_settings, tickets};

use super::source_release::release;
use super::tests::start;

fn apply(conn: &mut Connection, actor: &str, op: Operation, now: Timestamp) -> String {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![op],
    };
    match batch::apply(conn, &request, actor, "hub", now).unwrap() {
        BatchOutcome::Applied(reply) => reply.results[0].id.to_string(),
        BatchOutcome::Conflict(conflict) => panic!("conflict at {}", conflict.failed),
    }
}

/// Files a pending improvement as the hub's efficiency rules do.
fn pending_improvement(conn: &mut Connection, title: &str, now: Timestamp) -> String {
    let fields = TicketFields {
        title: Some(title.to_owned()),
        status: Some(Status::PendingApproval),
        labels: Some(vec!["improvement".to_owned()]),
        ..TicketFields::default()
    };
    let op = Operation::Create {
        fields,
        parent: None,
        deps: Vec::new(),
        request_key: None,
    };
    apply(conn, crate::rules::filing::ACTOR, op, now)
}

fn set_status(conn: &mut Connection, actor: &str, id: &str, status: Status, now: Timestamp) {
    let op = Operation::Update {
        id: id.to_owned(),
        set: Some(TicketFields {
            status: Some(status),
            ..TicketFields::default()
        }),
        add_labels: Vec::new(),
        remove_labels: Vec::new(),
        set_meta: BTreeMap::new(),
        unset_meta: Vec::new(),
    };
    apply(conn, actor, op, now);
}

fn status(conn: &Connection, id: &str) -> Status {
    tickets::load(conn, id).unwrap().unwrap().status
}

fn set_improvements(conn: &mut Connection, weight: u32, now: Timestamp) {
    let mut settings = fleet_settings::settings(conn).unwrap();
    let version = settings.version;
    settings.allocations = Some(Allocations {
        general: None,
        sources: BTreeMap::from([(Source::Improvement, weight)]),
    });
    assert!(matches!(
        fleet_settings::put(conn, version, settings, "operator", now).unwrap(),
        fleet_settings::PutOutcome::Stored(_)
    ));
}

#[test]
fn a_pending_improvement_inherits_the_general_pool_and_opens() {
    let mut conn = hub::open_in_memory().unwrap();
    let now = start().now();
    let filed = pending_improvement(&mut conn, "Add `ignite show`", now);
    assert_eq!(release(&mut conn, now).unwrap(), vec![filed.clone()]);
    assert_eq!(status(&conn, &filed), Status::Open);
}

#[test]
fn a_source_at_zero_keeps_its_tickets_parked_until_it_rises() {
    let mut conn = hub::open_in_memory().unwrap();
    let now = start().now();
    set_improvements(&mut conn, 0, now);
    let filed = pending_improvement(&mut conn, "Add `ignite show`", now);
    assert!(release(&mut conn, now).unwrap().is_empty());
    assert_eq!(status(&conn, &filed), Status::PendingApproval);
    set_improvements(&mut conn, 1, now);
    assert_eq!(release(&mut conn, now).unwrap(), vec![filed]);
}

#[test]
fn a_ticket_a_person_parked_stays_parked() {
    let mut conn = hub::open_in_memory().unwrap();
    let now = start().now();
    let filed = pending_improvement(&mut conn, "Add `ignite show`", now);
    set_status(&mut conn, "operator", &filed, Status::Open, now);
    set_status(&mut conn, "operator", &filed, Status::PendingApproval, now);
    assert!(release(&mut conn, now).unwrap().is_empty());
    assert_eq!(status(&conn, &filed), Status::PendingApproval);
}
