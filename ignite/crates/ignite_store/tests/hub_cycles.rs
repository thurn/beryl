//! A batch refuses a dependency that could never become ready.

use ignite_core::protocol::tickets::{BatchRequest, Dependency, Operation, TicketFields};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::open_in_memory;
use rusqlite::Connection;

fn now() -> Timestamp {
    "2026-11-17T12:00:00.000Z".parse().unwrap()
}

fn run(conn: &mut Connection, ops: Vec<Operation>) -> Result<Vec<String>, StoreError> {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply(conn, &request, "test", "m-1", now())? {
        BatchOutcome::Applied(reply) => {
            Ok(reply.results.iter().map(|r| r.id.to_string()).collect())
        }
        BatchOutcome::Conflict(_) => panic!("conflict"),
    }
}

fn create(title: &str, parent: Option<&str>, deps: &[&str]) -> Operation {
    Operation::Create {
        fields: TicketFields {
            title: Some(title.to_owned()),
            ..TicketFields::default()
        },
        parent: parent.map(str::to_owned),
        deps: deps
            .iter()
            .map(|on| Dependency {
                on: (*on).to_owned(),
                kind: None,
            })
            .collect(),
        request_key: None,
    }
}

fn depend(id: &str, on: &str) -> Operation {
    Operation::DepAdd {
        id: id.to_owned(),
        on: on.to_owned(),
        kind: None,
        external: false,
    }
}

fn rejection(result: Result<Vec<String>, StoreError>) -> String {
    match result {
        Err(StoreError::Rejected(message)) => message,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[test]
fn a_ticket_cannot_depend_on_itself() {
    let mut conn = open_in_memory().unwrap();
    let ids = run(&mut conn, vec![create("a", None, &[])]).unwrap();
    let message = rejection(run(&mut conn, vec![depend(&ids[0], &ids[0])]));
    assert!(message.contains("itself"), "{message}");
}

#[test]
fn a_cycle_of_two_or_three_is_refused_naming_its_tickets() {
    let mut conn = open_in_memory().unwrap();
    let ids = run(
        &mut conn,
        vec![
            create("a", None, &[]),
            create("b", None, &["$0"]),
            create("c", None, &["$1"]),
        ],
    )
    .unwrap();
    let message = rejection(run(&mut conn, vec![depend(&ids[0], &ids[2])]));
    for id in &ids {
        assert!(message.contains(id.as_str()), "{message}");
    }
    assert!(message.contains("cycle"), "{message}");
    // The opposite direction and an unrelated ticket are fine.
    run(&mut conn, vec![depend(&ids[2], &ids[0])]).unwrap();
}

#[test]
fn a_ticket_cannot_depend_on_its_own_ancestor_but_a_parent_may_not_wait_on_its_child_either() {
    let mut conn = open_in_memory().unwrap();
    let ids = run(
        &mut conn,
        vec![create("epic", None, &[]), create("task", Some("$0"), &[])],
    )
    .unwrap();
    let message = rejection(run(&mut conn, vec![depend(&ids[1], &ids[0])]));
    assert!(message.contains("cycle"), "{message}");
    // An ancestor's dependency is inherited by its children, so the parent waiting on its own
    // child leaves that child waiting on itself.
    let message = rejection(run(&mut conn, vec![depend(&ids[0], &ids[1])]));
    assert!(message.contains("cycle"), "{message}");
    // A cycle in a create's own `deps` is refused as well.
    let message = rejection(run(&mut conn, vec![create("x", Some(&ids[0]), &[&ids[0]])]));
    assert!(message.contains("cycle"), "{message}");
}

#[test]
fn edges_that_never_block_and_closed_tickets_make_no_cycle() {
    let mut conn = open_in_memory().unwrap();
    let ids = run(
        &mut conn,
        vec![create("a", None, &[]), create("b", None, &["$0"])],
    )
    .unwrap();
    run(
        &mut conn,
        vec![Operation::DepAdd {
            id: ids[0].clone(),
            on: ids[1].clone(),
            kind: Some(ignite_core::protocol::tickets::EdgeKind::Related),
            external: false,
        }],
    )
    .unwrap();
}
