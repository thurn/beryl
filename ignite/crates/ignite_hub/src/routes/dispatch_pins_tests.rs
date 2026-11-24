//! Dispatch and release honor independently inherited machine and provider pins.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::config::Os;
use ignite_core::protocol::tickets::BatchRequest;
use ignite_store::hub::{batch, tickets};

use super::tests::{at, claim_reserved, file, free, landed, open, post, register, release, start};

fn two_machines() -> Connection {
    let mut conn = open();
    register(&mut conn);
    let second = json!({"machine": "m-2", "name": "cygnus", "os": "windows",
                        "cores": 8, "memory_mb": 16384, "families": [], "version": "v"});
    assert_eq!(
        post(&mut conn, "/machines/register", &second, start()).0,
        200
    );
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
fn provider_and_machine_pins_filter_workers() {
    let mut conn = two_machines();
    let ids = file(
        &mut conn,
        &[
            ("on cygnus", &[], &[]),
            ("codex", &[], &[]),
            ("unpinned", &[], &[]),
        ],
    );
    set(&conn, ids[0].as_str(), "labels", r#"["machine:cygnus"]"#);
    set(&conn, ids[1].as_str(), "labels", r#"["provider:codex"]"#);
    // jarvis's Claude worker gets only the unpinned ticket.
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(first["ticket"], ids[2].as_str());
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1.0));
    assert_eq!(none, Value::Null);
    // jarvis's Codex worker gets the ticket pinned to its provider.
    let (_, codex) = post(
        &mut conn,
        "/next",
        &free_app(&["local_x"], "codex"),
        at(2.0),
    );
    assert_eq!(codex["ticket"], ids[1].as_str());
}

#[test]
fn whichever_machine_asks_first_can_take_an_unpinned_ticket() {
    for machine in ["m-1", "m-2"] {
        let mut conn = two_machines();
        let id = file(&mut conn, &[("unpinned", &[], &[])]).remove(0);
        let mut asking = free(machine, &["local_a"]);
        if machine == "m-2" {
            asking["os"] = json!("windows");
        }
        let (_, offer) = post(&mut conn, "/next", &asking, start());
        assert_eq!(offer["ticket"], id.as_str());
    }
}

#[test]
fn an_os_requirement_set_on_create_or_update_filters_dispatch() {
    for set_on_create in [true, false] {
        let mut conn = two_machines();
        let mut fields = json!({"title": "Native work", "priority": 0});
        if set_on_create {
            fields["needs_os"] = json!("macos");
        }
        let filed: BatchRequest = serde_json::from_value(json!({"ops": [
            {"op": "create", "fields": fields},
            {"op": "create", "fields": {"title": "Portable work", "priority": 1}},
        ]}))
        .unwrap();
        let batch::BatchOutcome::Applied(reply) =
            batch::apply(&mut conn, &filed, "planner", "m-1", start()).unwrap()
        else {
            panic!("filing conflicted");
        };
        let native = reply.results[0].id.as_str();
        let portable = reply.results[1].id.as_str();
        if !set_on_create {
            let updated: BatchRequest = serde_json::from_value(json!({"ops": [
                {"op": "update", "id": native, "set": {"needs_os": "macos"}},
            ]}))
            .unwrap();
            assert!(matches!(
                batch::apply(&mut conn, &updated, "planner", "m-1", at(1.0)).unwrap(),
                batch::BatchOutcome::Applied(_)
            ));
        }
        let ticket = tickets::load(&conn, native).unwrap().unwrap();
        assert_eq!(ticket.needs_os, Some(Os::Macos));

        let mut windows = free("m-2", &["local_windows"]);
        windows["os"] = json!("windows");
        let (status, offer) = post(&mut conn, "/next", &windows, at(2.0));
        assert_eq!(status, 200, "{offer}");
        assert_eq!(offer["ticket"], portable);
        windows["idle_workers"][0]["session"] = json!("local_windows_2");
        let (status, offer) = post(&mut conn, "/next", &windows, at(3.0));
        assert_eq!(status, 200, "{offer}");
        assert_eq!(offer, Value::Null);

        let (status, offer) = post(&mut conn, "/next", &free("m-1", &["local_mac"]), at(4.0));
        assert_eq!(status, 200, "{offer}");
        assert_eq!(offer["ticket"], native);
    }
}

#[test]
fn batch_filing_honors_legacy_os_labels_and_explicit_requirements() {
    let mut conn = two_machines();
    let request = super::Request {
        method: "POST".into(),
        path: "/batch".into(),
        query: Vec::new(),
        writer: Some(crate::auth::Writer {
            machine: "m-1".parse().unwrap(),
            actor: "planner".into(),
        }),
        body: serde_json::to_vec(&json!({"ops": [
            {"op": "create", "fields": {"title": "macOS work", "labels": ["needs:macos"]}},
            {"op": "create", "fields": {"title": "Windows work", "labels": ["needs:macos"],
                                        "needs_os": "windows"}},
            {"op": "update", "id": "$1", "add_labels": ["needs:macos", "tier:sonnet"]},
        ]}))
        .unwrap(),
        headers: Vec::new(),
    };
    let response = crate::routes::tickets::handle(&mut conn, &request, start()).unwrap();
    let reply: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response.status, 200, "{reply}");
    let macos = reply["results"][0]["id"].as_str().unwrap();
    let windows = reply["results"][1]["id"].as_str().unwrap();
    let ticket = tickets::load(&conn, macos).unwrap().unwrap();
    assert_eq!(ticket.needs_os, Some(Os::Macos));
    assert!(ticket.labels.is_empty());
    let ticket = tickets::load(&conn, windows).unwrap().unwrap();
    assert_eq!(ticket.needs_os, Some(Os::Windows));
    assert_eq!(ticket.labels, ["tier:sonnet"]);

    let mut request = free("m-2", &["local_windows"]);
    request["os"] = json!("windows");
    let (status, offer) = post(&mut conn, "/next", &request, at(1.0));
    assert_eq!(status, 200, "{offer}");
    assert_eq!(offer["ticket"], windows);
    request["idle_workers"][0]["session"] = json!("local_windows_2");
    let (status, offer) = post(&mut conn, "/next", &request, at(2.0));
    assert_eq!(status, 200, "{offer}");
    assert_eq!(offer, Value::Null);

    let (status, offer) = post(&mut conn, "/next", &free("m-1", &["local_mac"]), at(3.0));
    assert_eq!(status, 200, "{offer}");
    assert_eq!(offer["ticket"], macos);
}

#[test]
fn an_epics_machine_pin_applies_to_its_children() {
    let mut conn = two_machines();
    let id = file(&mut conn, &[("child", &[], &[])]).remove(0);
    let child = tickets::load(&conn, id.as_str()).unwrap().unwrap();
    let epic = child.parent.unwrap();
    set(&conn, epic.as_str(), "labels", r#"["machine:cygnus"]"#);
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(none, Value::Null);
    let mut windows = free("m-2", &["local_b"]);
    windows["os"] = json!("windows");
    let (_, offer) = post(&mut conn, "/next", &windows, at(1.0));
    assert_eq!(offer["ticket"], id.as_str());
}

#[test]
fn a_ticket_holding_master_falls_back_when_its_machine_is_silent() {
    let mut conn = two_machines();
    let ids = file(&mut conn, &[("fault on cygnus", &[], &[])]);
    let id = ids[0].as_str();
    set(
        &conn,
        id,
        "labels",
        r#"["justiciar","machine:cygnus","provider:claude"]"#,
    );
    set(&conn, id, "fallback", 1);
    let (_, early) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1799.0));
    assert_eq!(early, Value::Null);
    let (_, wrong_provider) = post(
        &mut conn,
        "/next",
        &free_app(&["local_x"], "codex"),
        at(1800.0),
    );
    assert_eq!(wrong_provider, Value::Null);
    let (_, taken) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1800.0));
    assert_eq!(taken["ticket"], id);
    // Released again and needing Windows, it has nowhere else to go.
    post(
        &mut conn,
        "/claims",
        &claim_reserved(&taken, "local_a"),
        at(1801.0),
    );
    post(&mut conn, "/release", &release(id, "local_a"), at(1802.0));
    set(&conn, id, "needs_os", "windows");
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1803.0));
    assert_eq!(none, Value::Null);
}

#[test]
fn a_release_for_a_machine_replaces_the_machine_pin() {
    let mut conn = two_machines();
    let id = file(&mut conn, &[("needs cygnus", &[], &[])]).remove(0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(1.0),
    );
    set(
        &conn,
        id.as_str(),
        "labels",
        r#"["machine:jarvis","provider:claude"]"#,
    );
    let mut body = release(id.as_str(), "local_a");
    body["needs"] = json!("cygnus");
    assert_eq!(post(&mut conn, "/release", &body, at(2.0)).0, 200);
    let ticket = tickets::load(&conn, id.as_str()).unwrap().unwrap();
    assert_eq!(ticket.labels, ["provider:claude", "machine:cygnus"]);
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(3.0));
    assert_eq!(none, Value::Null);
}

#[test]
fn a_lanes_next_ticket_pinned_elsewhere_is_not_handed_to_the_same_session() {
    let mut conn = two_machines();
    let ids = file(
        &mut conn,
        &[("first", &["lane:x"], &[]), ("second", &["lane:x"], &[0])],
    );
    set(
        &conn,
        ids[1].as_str(),
        "labels",
        r#"["lane:x","machine:cygnus"]"#,
    );
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
