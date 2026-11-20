//! A ticket released by different sessions is held back from the pool and goes to a justiciar
//! (see `dispatch_releases.rs`), until its text changes or a person releases it again.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use ignite_core::time::Timestamp;
use ignite_store::hub;

use super::tests::{at, claim, file, free, post, release};
use super::{Request, Response};
use crate::auth::Writer;

/// Claims `ticket` by name for `session` and releases it with `note` and `cause`.
fn work_and_release(
    conn: &mut Connection,
    ticket: &str,
    session: &str,
    note: &str,
    cause: Option<&str>,
    now: Timestamp,
) {
    let (status, reply) = post(conn, "/claims", &claim(ticket, session), now);
    assert_eq!(status, 200, "{reply}");
    let mut body = release(ticket, session);
    body["reason"] = json!(note);
    if let Some(cause) = cause {
        body["cause"] = json!(cause);
    }
    let (status, reply) = post(conn, "/release", &body, now.plus_seconds(1.0));
    assert_eq!(status, 200, "{reply}");
}

/// The ticket the pool would offer the idle worker `session` now, if any. Asking reserves it
/// for that worker, so each ask names a new one.
fn offered(conn: &mut Connection, session: &str, now: Timestamp) -> Option<String> {
    let (status, reply) = post(conn, "/next", &free("m-1", &[session]), now);
    assert_eq!(status, 200, "{reply}");
    reply["ticket"].as_str().map(str::to_owned)
}

/// Each justiciar ticket's ID and body, and how many comments it has.
fn justiciar_tickets(conn: &Connection) -> Vec<(String, String, i64)> {
    let mut statement = conn
        .prepare(
            "SELECT id, body, (SELECT count(*) FROM comments c WHERE c.ticket = t.id)
             FROM tickets t
             WHERE EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = 'justiciar')
             ORDER BY id",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// Sends a request to the ticket routes as `m-1`.
fn tickets_route(conn: &mut Connection, method: &str, path: &str, body: &Value) -> Response {
    let request = Request {
        method: method.into(),
        path: path.into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: MachineId::new("m-1").unwrap(),
            actor: "planner".into(),
        }),
        body: serde_json::to_vec(body).unwrap(),
        headers: Vec::new(),
    };
    crate::routes::tickets::handle(conn, &request, at(100.0)).expect("a ticket route")
}

fn shown(conn: &mut Connection, ticket: &str) -> Value {
    let response = tickets_route(conn, "GET", &format!("/tickets/{ticket}"), &Value::Null);
    assert_eq!(response.status, 200);
    serde_json::from_slice(&response.body).unwrap()
}

fn update(conn: &mut Connection, ticket: &str, set: &Value) {
    let batch = json!({"ops": [{"op": "update", "id": ticket, "set": set}]});
    let response = tickets_route(conn, "POST", "/batch", &batch);
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
}

#[test]
fn two_sessions_releasing_a_ticket_hold_it_back_and_file_one_justiciar_ticket() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("needs a fixture", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(
        &mut conn,
        id,
        "local_a",
        "The fixture is missing.",
        None,
        at(0.0),
    );
    // One session releasing twice is one worker's obstacle, not yet the ticket's.
    work_and_release(&mut conn, id, "local_a", "Still missing.", None, at(10.0));
    assert!(justiciar_tickets(&conn).is_empty());
    assert!(shown(&mut conn, id)["held_back"].is_null());
    work_and_release(
        &mut conn,
        id,
        "local_b",
        "No fixture here either.",
        Some("taken-back"),
        at(20.0),
    );

    let filed = justiciar_tickets(&conn);
    assert_eq!(filed.len(), 1, "{filed:?}");
    let (_, body, comments) = &filed[0];
    assert!(body.contains(id), "{body}");
    for note in ["> The fixture is missing.", "> No fixture here either."] {
        assert!(body.contains(note), "{body}");
    }
    assert_eq!(*comments, 0);
    let detail = shown(&mut conn, id);
    assert_eq!(detail["releases"]["count"], 3);
    assert!(
        detail["held_back"].as_str().unwrap().contains("2 sessions"),
        "{detail}"
    );
    // The justiciar ticket is offered; the held-back ticket is not.
    assert_eq!(
        offered(&mut conn, "local_idle", at(30.0)),
        Some(filed[0].0.clone())
    );
}

#[test]
fn a_third_session_comments_on_the_justiciar_ticket_instead_of_filing_another() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("impossible", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "first", None, at(0.0));
    work_and_release(&mut conn, id, "local_b", "second", None, at(10.0));
    // A session can still start the held-back ticket by name, as a justiciar does.
    work_and_release(&mut conn, id, "local_c", "third", None, at(20.0));
    let filed = justiciar_tickets(&conn);
    assert_eq!(filed.len(), 1, "{filed:?}");
    assert_eq!(filed[0].2, 1, "the third release adds one comment");
}

#[test]
fn a_release_at_the_plan_limit_or_with_needs_is_not_counted() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("ordinary", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "gave up", None, at(0.0));
    work_and_release(
        &mut conn,
        id,
        "local_b",
        "limit",
        Some("plan-limit"),
        at(10.0),
    );
    work_and_release(&mut conn, id, "local_c", "held", Some("cli-hold"), at(20.0));
    let (status, _) = post(&mut conn, "/claims", &claim(id, "local_d"), at(30.0));
    assert_eq!(status, 200);
    let mut body = release(id, "local_d");
    body["needs"] = json!("windows");
    assert_eq!(post(&mut conn, "/release", &body, at(31.0)).0, 200);
    assert!(justiciar_tickets(&conn).is_empty());
    assert_eq!(shown(&mut conn, id)["releases"]["count"], 1);
    update(&mut conn, id, &json!({"needs_os": null}));
    assert_eq!(
        offered(&mut conn, "local_idle", at(40.0)).as_deref(),
        Some(id)
    );
}

#[test]
fn editing_the_acceptance_offers_the_ticket_again() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("vague", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "first", None, at(0.0));
    work_and_release(&mut conn, id, "local_b", "second", None, at(10.0));
    // Only the justiciar ticket is left to offer, and taking it leaves nothing.
    let justiciar = justiciar_tickets(&conn).remove(0).0;
    assert_eq!(offered(&mut conn, "local_x", at(20.0)), Some(justiciar));
    assert_eq!(offered(&mut conn, "local_y", at(21.0)), None);

    update(
        &mut conn,
        id,
        &json!({"acceptance": "`cargo test` passes with a test of X."}),
    );
    let detail = shown(&mut conn, id);
    assert!(detail["held_back"].is_null(), "{detail}");
    assert!(detail["releases"].is_null(), "{detail}");
    assert_eq!(offered(&mut conn, "local_z", at(22.0)).as_deref(), Some(id));
}

#[test]
fn releasing_the_ticket_again_offers_it_again() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("blocked", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "first", None, at(0.0));
    work_and_release(&mut conn, id, "local_b", "second", None, at(10.0));
    // `ignite ticket update <id> --release` sends the status `open`.
    update(&mut conn, id, &json!({"status": "open"}));
    assert!(shown(&mut conn, id)["held_back"].is_null());
}
