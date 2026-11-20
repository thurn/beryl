//! `POST /reverted` against an in-memory hub database.

use rusqlite::Connection;
use serde_json::json;

use ignite_core::ids::{CheckName, Commit, TicketId};
use ignite_core::protocol::hub::Reverted;
use ignite_core::protocol::tickets::{
    EdgeKind, Operation, Status, Ticket, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_store::hub::tickets;

use super::filing::Author;
use super::reverted::{self, KEY};
use super::tests::{all_tickets, apply, create, start};

const AUTHOR: Author<'static> = Author {
    actor: "m-1",
    machine: "m-1",
};

fn report(ticket: Option<&str>, revert: &str) -> Reverted {
    Reverted {
        ticket: ticket.map(|t| TicketId::new(t).unwrap()),
        landed: vec![Commit::new("def5678").unwrap()],
        revert: Commit::new(revert).unwrap(),
        checks: vec![CheckName::new("rust:a::b").unwrap()],
        output: "error[E0308]: mismatched types".to_owned(),
        search: CulpritSearch::OneCandidate,
        machine: "m-1".to_owned(),
    }
}

fn setup() -> (Connection, Timestamp, String) {
    let clock = start();
    let now = clock.now();
    let mut conn = ignite_store::hub::open_in_memory().unwrap();
    let ids = apply(
        &mut conn,
        vec![create("Landed", TicketType::Task, None, &[])],
        now,
    );
    let id = ids[0].clone();
    apply(
        &mut conn,
        vec![Operation::Close {
            id: id.clone(),
            reason: "landed".to_owned(),
            include_ready: false,
        }],
        now,
    );
    (conn, now, id)
}

fn load(conn: &Connection, id: &str) -> Ticket {
    tickets::load(conn, id).unwrap().unwrap()
}

use ignite_core::traits::Clock;

#[test]
fn a_reverted_landing_reopens_its_ticket_at_priority_zero() {
    let (mut conn, now, id) = setup();
    let filed = reverted::file(&mut conn, AUTHOR, &report(Some(&id), "abc1234"), now).unwrap();
    assert_eq!(filed, id);
    let ticket = load(&conn, &id);
    assert_eq!(ticket.status, Status::Open);
    assert_eq!(ticket.priority, 0);
    assert_eq!(ticket.metadata[KEY]["revert"], json!("abc1234"));
    assert_eq!(ticket.metadata[KEY]["landed"], json!(["def5678"]));
    assert_eq!(ticket.metadata[KEY]["checks"], json!(["rust:a::b"]));
    let (text,): (String,) = conn
        .query_row("SELECT text FROM comments WHERE ticket = ?1", [&id], |r| {
            Ok((r.get(0)?,))
        })
        .unwrap();
    assert!(text.contains("rust:a::b"), "{text}");
    assert!(text.contains("mismatched types"), "{text}");
    assert!(
        text.contains("apply your work again with: git revert --no-edit abc1234"),
        "{text}"
    );
}

#[test]
fn the_same_revert_reported_twice_changes_nothing() {
    let (mut conn, now, id) = setup();
    reverted::file(&mut conn, AUTHOR, &report(Some(&id), "abc1234"), now).unwrap();
    let before = load(&conn, &id);
    let comments = |conn: &Connection| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM comments WHERE ticket = ?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap()
    };
    let count = comments(&conn);
    reverted::file(
        &mut conn,
        AUTHOR,
        &report(Some(&id), "abc1234"),
        now.plus_seconds(60.0),
    )
    .unwrap();
    assert_eq!(load(&conn, &id), before);
    assert_eq!(comments(&conn), count);
}

#[test]
fn commits_no_landing_recorded_get_a_new_priority_zero_ticket() {
    let (mut conn, now, _) = setup();
    let first = reverted::file(&mut conn, AUTHOR, &report(None, "abc1234"), now).unwrap();
    let again = reverted::file(&mut conn, AUTHOR, &report(None, "abc1234"), now).unwrap();
    assert_eq!(first, again);
    let ticket = load(&conn, &first);
    assert_eq!((ticket.status, ticket.priority), (Status::Open, 0));
    assert!(ticket.body.contains("git revert --no-edit abc1234"));
    assert_eq!(ticket.metadata[KEY]["revert"], json!("abc1234"));
    assert_eq!(all_tickets(&conn, now).len(), 2);
}

#[test]
fn a_session_holding_a_dependent_ticket_is_told_to_release_it() {
    let (mut conn, now, id) = setup();
    let dependent = apply(
        &mut conn,
        vec![Operation::Create {
            fields: TicketFields {
                title: Some("Dependent".to_owned()),
                status: Some(Status::InProgress),
                assignee: Some(Some("local_w1".to_owned())),
                ..TicketFields::default()
            },
            parent: None,
            deps: vec![ignite_core::protocol::tickets::Dependency {
                on: id.clone(),
                kind: Some(EdgeKind::Blocks),
            }],
            request_key: None,
        }],
        now,
    )[0]
    .clone();
    let idle = apply(
        &mut conn,
        vec![create("Idle", TicketType::Task, None, &[&id])],
        now,
    )[0]
    .clone();
    reverted::file(&mut conn, AUTHOR, &report(Some(&id), "abc1234"), now).unwrap();
    let held = load(&conn, &dependent);
    assert_eq!(held.metadata["tell"], json!(1));
    let (text,): (String,) = conn
        .query_row(
            "SELECT text FROM comments WHERE ticket = ?1",
            [&dependent],
            |r| Ok((r.get(0)?,)),
        )
        .unwrap();
    assert!(
        text.contains("Release this ticket with `ignite release`"),
        "{text}"
    );
    assert!(
        text.contains(&format!("handed out again when {id} lands")),
        "{text}"
    );
    // A dependent nobody holds gets nothing: it waits for the reopened ticket by itself.
    assert!(!load(&conn, &idle).metadata.contains_key("tell"));
}
