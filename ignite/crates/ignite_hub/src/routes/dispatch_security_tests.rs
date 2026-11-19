//! Who may register, claim and name tickets how: the token decides the machine, and a ticket may
//! be named by any form `GET /tickets/<name>` accepts.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use ignite_store::hub::{self, tickets, tokens};

use super::tests::{at, claim, file, post, start};
use super::{Request, handle};
use crate::auth::Writer;

fn registration(machine: &str) -> Value {
    json!({"machine": machine, "name": "host", "os": "macos", "cores": 8,
           "memory_mb": 16384, "families": [], "version": "d97ed0e"})
}

/// `POST /machines/register` with the given writer and `Authorization` header.
fn register_as(
    conn: &mut Connection,
    machine: &str,
    writer: Option<&str>,
    authorization: Option<&str>,
) -> (u16, Value) {
    let request = Request {
        method: "POST".into(),
        path: "/machines/register".into(),
        query: Vec::new(),
        writer: writer.map(|machine| Writer {
            machine: MachineId::new(machine).unwrap(),
            actor: "test".into(),
        }),
        body: serde_json::to_vec(&registration(machine)).unwrap(),
        headers: authorization
            .map(|value| vec![("authorization".to_owned(), value.to_owned())])
            .unwrap_or_default(),
    };
    let response = handle(conn, &request, start()).unwrap();
    (
        response.status,
        serde_json::from_slice(&response.body).unwrap(),
    )
}

#[test]
fn registering_a_machine_that_has_a_token_needs_that_token() {
    let mut conn = hub::open_in_memory().unwrap();
    let (status, first) = register_as(&mut conn, "m-9", None, None);
    assert_eq!(status, 200, "{first}");
    let token = first["token"].as_str().unwrap().to_owned();

    // Nobody else can replace m-9's token: not an anonymous caller, not another machine.
    assert_eq!(register_as(&mut conn, "m-9", None, None).0, 401);
    let other = tokens::issue(&conn, &MachineId::new("m-8").unwrap()).unwrap();
    assert_eq!(register_as(&mut conn, "m-9", Some("m-8"), None).0, 401);
    assert_eq!(
        register_as(&mut conn, "m-9", None, Some(&format!("Bearer {other}"))).0,
        401
    );
    assert_eq!(
        tokens::verify(&conn, &token).unwrap().unwrap().as_str(),
        "m-9",
        "the refused calls left the token alone"
    );

    // The machine itself, sending only its bearer as `ignite install` does, keeps its token, so
    // the daemon and verb processes that hold it go on writing.
    let (status, again) = register_as(&mut conn, "m-9", None, Some(&format!("Bearer {token}")));
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["token"], first["token"]);
    assert_eq!(
        tokens::verify(&conn, &token).unwrap().unwrap().as_str(),
        "m-9"
    );
}

#[test]
fn a_body_naming_another_machine_than_the_token_is_refused() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("one", &[], &[])]);
    let body = json!({"ticket": ids[0].as_str(), "machine": "m-2", "session": "local_x",
                      "tree": "tree-01"});
    let request = |machine: &str| Request {
        method: "POST".into(),
        path: "/claims".into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: MachineId::new(machine).unwrap(),
            actor: "test".into(),
        }),
        body: serde_json::to_vec(&body).unwrap(),
        headers: Vec::new(),
    };
    let refused = handle(&mut conn, &request("m-1"), start()).unwrap();
    assert_eq!(refused.status, 403);
    let ticket = tickets::load(&conn, ids[0].as_str()).unwrap().unwrap();
    assert_eq!(ticket.assignee, None);
    assert_eq!(
        handle(&mut conn, &request("m-2"), at(1.0)).unwrap().status,
        200
    );
}

#[test]
fn a_claim_records_the_session_and_tree_and_drops_the_reservation_together() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("one", &[], &[])]);
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim(ids[0].as_str(), "local_a"),
        start(),
    );
    assert_eq!(status, 200, "{brief}");
    let ticket = tickets::load(&conn, ids[0].as_str()).unwrap().unwrap();
    assert_eq!(ticket.assignee.as_deref(), Some("local_a"));
    assert_eq!(
        ticket.session.as_ref().map(ToString::to_string).as_deref(),
        Some("local_a")
    );
    assert_eq!(
        ticket.tree.as_ref().map(ToString::to_string).as_deref(),
        Some("tree-01")
    );
    // A second session is refused and the first claim is untouched.
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim(ids[0].as_str(), "local_b"),
        at(1.0),
    );
    assert_eq!(status, 409);
    let ticket = tickets::load(&conn, ids[0].as_str()).unwrap().unwrap();
    assert_eq!(ticket.assignee.as_deref(), Some("local_a"));
}

#[test]
fn a_ticket_is_claimed_by_its_id_with_or_without_the_t_or_by_alias() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[("one", &[], &[]), ("two", &[], &[]), ("three", &[], &[])],
    );
    conn.execute(
        "INSERT INTO aliases (alias, id) VALUES ('old-name', ?1)",
        [ids[2].as_str()],
    )
    .unwrap();
    let names = [
        // The spelling from before IDs were stored in capitals: `T` and the rest in lowercase.
        (format!("T{}", ids[0].as_str()[1..].to_lowercase()), 0),
        (ids[1].to_string(), 1),
        ("old-name".to_owned(), 2),
    ];
    for (name, index) in names {
        let (status, brief) = post(&mut conn, "/claims", &claim(&name, "local_a"), start());
        assert_eq!(status, 200, "{name}: {brief}");
        assert_eq!(brief["ticket"]["id"], ids[index].as_str());
        // Giving it back by the same spelling works too.
        let release = json!({"ticket": name, "machine": "m-1", "session": "local_a",
                             "reason": "test"});
        assert_eq!(
            post(&mut conn, "/release", &release, at(1.0)).0,
            200,
            "{name}"
        );
    }
    let (status, _) = post(&mut conn, "/claims", &claim("Tzzzz.9", "local_a"), at(2.0));
    assert_eq!(status, 404);
}

#[test]
fn a_machine_may_pause_itself_or_every_machine_but_not_another_alone() {
    let mut conn = hub::open_in_memory().unwrap();
    let pause_as = |conn: &mut Connection, writer: &str, machine: Value| {
        let request = Request {
            method: "POST".into(),
            path: "/pause".into(),
            query: Vec::new(),
            writer: Some(Writer {
                machine: MachineId::new(writer).unwrap(),
                actor: "test".into(),
            }),
            body: serde_json::to_vec(&json!({"machine": machine, "paused": true})).unwrap(),
            headers: Vec::new(),
        };
        handle(conn, &request, start()).unwrap().status
    };
    assert_eq!(pause_as(&mut conn, "m-1", json!("m-2")), 403);
    assert_eq!(pause_as(&mut conn, "m-1", json!("m-1")), 200);
    assert_eq!(pause_as(&mut conn, "m-1", Value::Null), 200);
}
