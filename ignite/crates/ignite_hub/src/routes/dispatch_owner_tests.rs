//! Dispatch by owner (see `ignite_core::owner`): a ticket goes only to workers of its own owner
//! or its nearest ancestor's, a ticket with neither goes to any worker, and a ticket that holds
//! `master` back falls back when its owner's machine goes silent.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::config::Os;
use ignite_core::owner::{RoutedMachine, Routing};
use ignite_store::hub::{self, owners, tickets};

use super::tests::{at, claim_reserved, file, free, landed, post, register, release, start};

fn routing() -> Routing {
    let machine = |name: &str, os: Os| RoutedMachine {
        name: name.to_owned(),
        os: Some(os),
    };
    Routing {
        machines: vec![machine("cygnus", Os::Windows), machine("jarvis", Os::Macos)],
        apps: vec!["claude".to_owned(), "codex".to_owned()],
        default_owner: None,
    }
}

fn hub_with_routing() -> Connection {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    owners::record_routing(&conn, &routing()).unwrap();
    conn
}

fn set(conn: &Connection, id: &str, column: &str, value: impl rusqlite::ToSql) {
    let sql = format!("UPDATE tickets SET {column} = ?2 WHERE id = ?1");
    conn.execute(&sql, rusqlite::params![id, value]).unwrap();
}

/// A machine's idle workers, all of `app`.
fn free_app(sessions: &[&str], app: &str) -> Value {
    let mut body = free("m-1", sessions);
    for worker in body["idle_workers"].as_array_mut().unwrap() {
        worker["app"] = json!(app);
    }
    body
}

#[test]
fn a_ticket_goes_only_to_workers_of_its_owner() {
    let mut conn = hub_with_routing();
    let ids = file(
        &mut conn,
        &[
            ("on cygnus", &[], &[]),
            ("jarvis codex", &[], &[]),
            ("unowned", &[], &[]),
        ],
    );
    set(&conn, ids[0].as_str(), "owner", "cygnus/codex");
    set(&conn, ids[1].as_str(), "owner", "jarvis/codex");
    // jarvis's Claude worker gets only the ticket nobody owns.
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(first["ticket"], ids[2].as_str());
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1.0));
    assert_eq!(none, Value::Null);
    // jarvis's Codex worker gets the ticket owned by jarvis/codex, never cygnus's.
    let (_, codex) = post(
        &mut conn,
        "/next",
        &free_app(&["local_x"], "codex"),
        at(2.0),
    );
    assert_eq!(codex["ticket"], ids[1].as_str());
}

#[test]
fn an_unowned_ticket_goes_only_to_the_default_owner() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let routing = Routing {
        default_owner: Some("jarvis/claude".parse().unwrap()),
        ..routing()
    };
    owners::record_routing(&conn, &routing).unwrap();
    let ids = file(&mut conn, &[("unowned", &[], &[])]);
    // A Codex worker on the same machine is not the default owner, so it gets nothing.
    let (_, none) = post(
        &mut conn,
        "/next",
        &free_app(&["local_x"], "codex"),
        start(),
    );
    assert_eq!(none, Value::Null);
    let (_, claude) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1.0));
    assert_eq!(claude["ticket"], ids[0].as_str());
}

#[test]
fn a_ticket_follows_its_epics_owner_and_goes_to_anyone_without_one() {
    let mut conn = hub_with_routing();
    let ids = file(&mut conn, &[("child", &[], &[])]);
    let child = tickets::load(&conn, ids[0].as_str()).unwrap().unwrap();
    // The file helper puts tickets under an active epic; owning the epic routes the child.
    let epic = child.parent.clone().expect("filed under an epic");
    set(&conn, epic.as_str(), "owner", "jarvis/codex");
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(none, Value::Null);
    let (_, codex) = post(
        &mut conn,
        "/next",
        &free_app(&["local_x"], "codex"),
        at(1.0),
    );
    assert_eq!(codex["ticket"], ids[0].as_str());
    // Under an epic with no owner, a Claude worker may take it too.
    let mut conn = hub_with_routing();
    let ids = file(&mut conn, &[("child", &[], &[])]);
    let (_, claude) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(2.0));
    assert_eq!(claude["ticket"], ids[0].as_str());
}

#[test]
fn a_ticket_holding_master_falls_back_when_its_machine_is_silent() {
    let mut conn = hub_with_routing();
    let ids = file(&mut conn, &[("regression seen on cygnus", &[], &[])]);
    let id = ids[0].as_str();
    set(&conn, id, "owner", "cygnus");
    set(&conn, id, "fallback", 1);
    // cygnus has never been heard from, so any app on a machine that is heard from may take it.
    let (_, taken) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(taken["ticket"], id);
    // Released again and needing Windows, it has nowhere else to go.
    post(
        &mut conn,
        "/claims",
        &claim_reserved(&taken, "local_a"),
        at(1.0),
    );
    post(&mut conn, "/release", &release(id, "local_a"), at(2.0));
    set(&conn, id, "needs_os", "windows");
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(3.0));
    assert_eq!(none, Value::Null);
}

#[test]
fn a_release_for_a_machine_makes_it_the_owner() {
    let mut conn = hub_with_routing();
    let id = file(&mut conn, &[("needs cygnus", &[], &[])]).remove(0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(1.0),
    );
    let mut body = release(id.as_str(), "local_a");
    body["needs"] = json!("cygnus");
    assert_eq!(post(&mut conn, "/release", &body, at(2.0)).0, 200);
    let ticket = tickets::load(&conn, id.as_str()).unwrap().unwrap();
    assert_eq!(ticket.owner, Some("cygnus".parse().unwrap()));
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(3.0));
    assert_eq!(none, Value::Null);
}

#[test]
fn a_lanes_next_ticket_owned_elsewhere_is_not_handed_to_the_same_session() {
    let mut conn = hub_with_routing();
    let ids = file(
        &mut conn,
        &[("first", &["lane:x"], &[]), ("second", &["lane:x"], &[0])],
    );
    set(&conn, ids[1].as_str(), "owner", "cygnus/codex");
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reservation["ticket"], ids[0].as_str());
    post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(5.0),
    );
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", "done"),
        at(60.0),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["next"], Value::Null, "{reply}");
}
