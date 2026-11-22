use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_core::ids::TicketId;
use ignite_core::manifest::Manifest;
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, placement_deferrals};

use super::super::placement_tests::{capacity, record};
use super::super::tests::{at, file, free, post, start};
use super::publish;

fn declare(conn: &Connection, platforms: &str) {
    let manifest = format!(
        r#"
[family.native]
kind = "regression"
adapter = "jsonl"
platforms = {platforms}
list = "list"
run = "run"
inputs = ["engine/src/**"]

[family.probe]
kind = "exploration"
adapter = "jsonl"
platforms = "any"
list = "list"
run = "run"
next = "next"
inputs = ["engine/src/**"]
"#
    );
    let manifest = Manifest::from_files(&[(PathBuf::from("engine.toml"), manifest)]).unwrap();
    publish(conn, &manifest).unwrap();
}

fn register(conn: &mut Connection, machine: &str, os: &str, families: &[&str]) {
    let body = json!({
        "machine": machine, "name": machine, "os": os, "families": families,
        "cores": 20, "memory_mb": 16384, "version": "v",
    });
    assert_eq!(post(conn, "/machines/register", &body, start()).0, 200);
}

fn heartbeat(conn: &mut Connection, machine: &str, families: &[&str]) {
    let body = json!({
        "machine": machine, "families": families, "version": "v",
        "workers": [], "cli_apps": ["claude"],
    });
    assert_eq!(post(conn, "/machines/heartbeat", &body, start()).0, 200);
}

fn ticket(conn: &mut Connection) -> TicketId {
    let ticket = file(conn, &[("change the engine", &[], &[])]).remove(0);
    conn.execute(
        "UPDATE tickets SET acceptance = 'Exercise `engine/src/new.rs`.' WHERE id = ?1",
        [ticket.as_str()],
    )
    .unwrap();
    ticket
}

fn ask(conn: &mut Connection, free: &Value, now: Timestamp) -> Value {
    let (status, reply) = post(conn, "/next", free, now);
    assert_eq!(status, 200, "{reply}");
    reply
}

#[test]
fn ordinary_dispatch_uses_the_registered_family_list_and_the_reviewed_inputs() {
    let mut conn = hub::open_in_memory().unwrap();
    declare(&conn, "\"any\"");
    register(&mut conn, "m-1", "macos", &["portable"]);
    let ticket = ticket(&mut conn);
    let mut request = free("m-1", &["local_a"]);
    request["explain"] = json!(true);
    request["families"] = json!(["native"]);
    let refused = ask(&mut conn, &request, at(1.0));
    assert!(
        refused["nothing"]
            .as_str()
            .unwrap()
            .contains("check families")
    );

    heartbeat(&mut conn, "m-1", &["native"]);
    request["families"] = json!(["portable"]);
    let reserved = ask(&mut conn, &request, at(2.0));
    assert_eq!(reserved["ticket"], ticket.as_str(), "{reserved}");
}

#[test]
fn a_family_platform_applies_even_on_a_machine_with_an_unrestricted_family_list() {
    let mut conn = hub::open_in_memory().unwrap();
    declare(&conn, "[\"macos\"]");
    register(&mut conn, "m-win", "windows", &[]);
    register(&mut conn, "m-mac", "macos", &[]);
    let ticket = ticket(&mut conn);

    let mut windows = free("m-win", &["local_a"]);
    windows["os"] = json!("windows");
    assert_eq!(ask(&mut conn, &windows, at(1.0)), Value::Null);

    let mac = ask(&mut conn, &free("m-mac", &["local_b"]), at(2.0));
    assert_eq!(mac["ticket"], ticket.as_str(), "{mac}");
}

#[test]
fn a_warm_machine_without_the_family_cannot_defer_a_suitable_asker() {
    let mut conn = hub::open_in_memory().unwrap();
    declare(&conn, "\"any\"");
    register(&mut conn, "m-1", "macos", &["native"]);
    register(&mut conn, "m-2", "macos", &["portable"]);
    let ticket = ticket(&mut conn);
    record(&conn, "m-2", &capacity(true), start());

    let mut cold = free("m-1", &[]);
    cold["room"] = json!({"start": 2, "providers": [{"name": "claude", "tiers": ["sonnet"]}]});
    cold["pressure"] = json!(DispatchPressure {
        build_slots: 6,
        build_slots_free: 6,
        outputs: capacity(false).outputs,
        ..DispatchPressure::default()
    });
    let reserved = ask(&mut conn, &cold, at(1.0));
    assert_eq!(reserved["ticket"], ticket.as_str(), "{reserved}");
    assert_eq!(placement_deferrals::first(&conn, &ticket).unwrap(), None);
}

#[test]
fn only_a_suitable_active_machine_blocks_checks_only_fallback() {
    let mut conn = hub::open_in_memory().unwrap();
    declare(&conn, "\"any\"");
    register(&mut conn, "m-1", "macos", &["native"]);
    register(&mut conn, "m-2", "macos", &["portable"]);
    heartbeat(&mut conn, "m-1", &["native"]);
    assert_eq!(
        post(
            &mut conn,
            "/pause",
            &json!({"machine": "m-2", "paused": true}),
            start()
        )
        .0,
        200
    );
    let ticket = file(&mut conn, &[("repair a failing check", &[], &[])]).remove(0);
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('native:example', ?1, 'b1', 'a1', 'open')",
        [ticket.as_str()],
    )
    .unwrap();
    let request = free("m-2", &["local_a"]);

    heartbeat(&mut conn, "m-2", &["native"]);
    assert_eq!(ask(&mut conn, &request, at(1.0)), Value::Null);

    heartbeat(&mut conn, "m-1", &["portable"]);
    heartbeat(&mut conn, "m-2", &["portable"]);
    assert_eq!(ask(&mut conn, &request, at(2.0)), Value::Null);

    heartbeat(&mut conn, "m-2", &["native"]);
    let reserved = ask(&mut conn, &request, at(3.0));
    assert_eq!(reserved["ticket"], ticket.as_str(), "{reserved}");
    assert_eq!(reserved["master_blocker"], true);
}
