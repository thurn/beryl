use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use ignite_core::protocol::hub::CheckState;
use ignite_core::protocol::tickets::{BatchRequest, Operation, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, batch, check_state, checks, rows, tickets};

use super::super::Request;
use super::handle;
use crate::auth::Writer;

fn now() -> Timestamp {
    "2026-11-17T12:00:00.000Z".parse().unwrap()
}

fn request(method: &str, path: &str, body: &Value, writer: bool) -> Request {
    Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: Vec::new(),
        writer: writer.then(|| Writer {
            machine: MachineId::new("m-1").unwrap(),
            actor: "local_abc".to_owned(),
        }),
        body: serde_json::to_vec(body).unwrap(),
        headers: Vec::new(),
    }
}

fn send(
    conn: &mut Connection,
    method: &str,
    path: &str,
    body: &Value,
    writer: bool,
) -> (u16, Value) {
    let response = handle(conn, &request(method, path, body, writer), now()).expect("owned path");
    (
        response.status,
        serde_json::from_slice(&response.body).unwrap(),
    )
}

/// A hub with the check `rust:a` switched off by the flaky upkeep, which filed a ticket for its
/// two flaky records; returns the ticket's ID.
fn switched_off() -> (Connection, String) {
    let mut conn = hub::open_in_memory().unwrap();
    checks::upsert(
        &mut conn,
        &[rows::Check {
            name: "rust:a".to_owned(),
            family: "rust".to_owned(),
            kind: "regression".to_owned(),
            state: "on".to_owned(),
            check_group: "group".to_owned(),
        }],
    )
    .unwrap();
    let create = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Create {
            fields: TicketFields {
                title: Some("Flaky check switched off: rust:a".to_owned()),
                ticket_type: Some(TicketType::Bug),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        }],
    };
    let batch::BatchOutcome::Applied(reply) =
        batch::apply(&mut conn, &create, "ignite-upkeep", "hub", now()).unwrap()
    else {
        panic!("not created");
    };
    let ticket = reply.results[0].id.to_string();
    for commit in ["aaaa", "bbbb"] {
        conn.execute(
            "INSERT INTO flaky (check_name, hash, commit_sha, machine, at, filed)
             VALUES ('rust:a', ?1, ?2, 'm-1', '2026-11-16T12:00:00.000Z', ?3)",
            rusqlite::params!["1".repeat(64), commit, ticket],
        )
        .unwrap();
    }
    checks::set_state(&conn, "rust:a", CheckState::Off).unwrap();
    (conn, ticket)
}

#[test]
fn switching_a_check_on_records_who_did_it_on_its_flaky_ticket() {
    let (mut conn, ticket) = switched_off();
    let (status, reply) = send(
        &mut conn,
        "POST",
        "/checks/state",
        &json!({"check": "rust:a", "state": "on", "note": "replaced with a unit test"}),
        true,
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(
        reply,
        json!({"check": "rust:a", "previous": "off", "state": "on", "ticket": ticket})
    );
    assert_eq!(
        check_state::state(&conn, "rust:a").unwrap(),
        Some(CheckState::On)
    );
    let comments = tickets::comments(&conn, &ticket).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "local_abc");
    assert!(
        comments[0]
            .text
            .starts_with("local_abc on machine m-1 switched rust:a on by hand (it was off)."),
        "{}",
        comments[0].text
    );
    assert!(comments[0].text.ends_with("replaced with a unit test"));

    let (status, listed) = send(&mut conn, "GET", "/checks/flaky", &json!(null), false);
    assert_eq!(status, 200);
    let check = &listed["checks"][0];
    assert_eq!(
        (&check["check"], &check["state"], &check["ticket"]),
        (&json!("rust:a"), &json!("on"), &json!(ticket))
    );
    assert_eq!(check["switched"]["actor"], json!("local_abc"));
}

#[test]
fn switching_to_the_state_it_has_records_nothing() {
    let (mut conn, ticket) = switched_off();
    let (status, reply) = send(
        &mut conn,
        "POST",
        "/checks/state",
        &json!({"check": "rust:a", "state": "off"}),
        true,
    );
    assert_eq!(status, 200);
    assert_eq!(reply["ticket"], Value::Null);
    assert!(tickets::comments(&conn, &ticket).unwrap().is_empty());
}

#[test]
fn a_switch_needs_a_token_a_known_check_and_on_or_off() {
    let (mut conn, _) = switched_off();
    let on = json!({"check": "rust:a", "state": "on"});
    assert_eq!(send(&mut conn, "POST", "/checks/state", &on, false).0, 401);
    assert_eq!(send(&mut conn, "GET", "/checks/state", &on, true).0, 405);
    let unknown = json!({"check": "rust:nope", "state": "on"});
    assert_eq!(
        send(&mut conn, "POST", "/checks/state", &unknown, true).0,
        404
    );
    let uncached = json!({"check": "rust:a", "state": "uncached"});
    assert_eq!(
        send(&mut conn, "POST", "/checks/state", &uncached, true).0,
        400
    );
    assert_eq!(
        check_state::state(&conn, "rust:a").unwrap(),
        Some(CheckState::Off),
        "a refused switch changes nothing"
    );
}
