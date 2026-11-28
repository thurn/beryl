//! A ticket that holds `master` back goes to the fastest machine that can take it, and to the
//! culprit's machine only when that machine is nearly as fast.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::time::Timestamp;
use ignite_store::hub;

use super::placement_tests::{capacity, record};
use super::tests::{at, file, free, open, post, register, start};

/// jarvis (`m-1`) and multivac (`m-2`) run macOS and cygnus (`m-3`) runs Windows. Each has room
/// for workers at `now`, and the operator has set the given speeds.
fn fleet(speeds: [&str; 3], now: Timestamp) -> Connection {
    let mut conn = open();
    register(&mut conn);
    for (id, name, os) in [("m-2", "multivac", "macos"), ("m-3", "cygnus", "windows")] {
        let machine = json!({"machine": id, "name": name, "os": os, "cores": 20,
                             "memory_mb": 16384, "families": [], "version": "v"});
        assert_eq!(post(&mut conn, "/machines/register", &machine, start()).0, 200);
    }
    let mut settings = hub::fleet_settings::settings(&conn).unwrap();
    let version = settings.version;
    for (name, speed) in ["jarvis", "multivac", "cygnus"].into_iter().zip(speeds) {
        settings.machines.entry(name.to_owned()).or_default().speed = Some(speed.parse().unwrap());
    }
    hub::fleet_settings::put(&mut conn, version, settings, "operator", start()).unwrap();
    for machine in ["m-1", "m-2", "m-3"] {
        record(&conn, machine, &capacity(true), now);
    }
    conn
}

/// Records `id` as the ticket of a failing check, which makes it a regression to dispatch.
fn failing(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('rust:a', ?1, 'abcdef1', 'abcdef0', 'open')",
        [id],
    )
    .unwrap();
}

/// A regression whose culprit landed on multivac with Claude.
fn regression(conn: &mut Connection, labels: &str) -> String {
    let id = file(conn, &[("regression", &[], &[])]).remove(0);
    failing(conn, id.as_str());
    let affinity = json!({"culprit_affinity": {
        "landing": {"machine": "m-2", "provider": "claude"},
        "machine_label": "machine:multivac",
        "added_label": false,
    }});
    conn.execute(
        "UPDATE tickets SET labels = ?2, metadata = ?3 WHERE id = ?1",
        rusqlite::params![id.as_str(), labels, affinity.to_string()],
    )
    .unwrap();
    id.to_string()
}

fn ask(conn: &mut Connection, machine: &str, now: Timestamp) -> Value {
    let mut body = free(machine, &["local_a"]);
    if machine == "m-3" {
        body["os"] = json!("windows");
    }
    let (status, reply) = post(conn, "/next", &body, now);
    assert_eq!(status, 200, "{reply}");
    reply
}

#[test]
fn a_regression_whose_culprit_machine_is_slow_goes_to_the_fastest_free_machine() {
    let mut conn = fleet(["1.0", "0.4", "1.0"], at(1.0));
    let id = regression(&mut conn, r#"["regression"]"#);
    assert_eq!(ask(&mut conn, "m-2", at(1.0)), Value::Null);
    assert_eq!(ask(&mut conn, "m-1", at(2.0))["ticket"], id);
}

#[test]
fn a_ticket_passed_over_for_a_minute_goes_to_whichever_machine_asks() {
    let mut conn = fleet(["1.0", "0.4", "1.0"], at(1.0));
    let id = regression(&mut conn, r#"["regression"]"#);
    assert_eq!(ask(&mut conn, "m-2", at(1.0)), Value::Null);
    record(&conn, "m-1", &capacity(true), at(61.0));
    assert_eq!(ask(&mut conn, "m-2", at(61.0))["ticket"], id);
}

#[test]
fn the_culprit_machine_keeps_a_regression_when_nearly_as_fast() {
    let mut conn = fleet(["1.0", "0.8", "1.0"], at(1.0));
    let id = regression(&mut conn, r#"["regression"]"#);
    assert_eq!(ask(&mut conn, "m-1", at(1.0)), Value::Null);
    let reply = ask(&mut conn, "m-2", at(2.0));
    assert_eq!(reply["ticket"], id);
}

#[test]
fn a_regression_that_needs_windows_stays_on_a_windows_machine() {
    let mut conn = fleet(["1.0", "1.0", "0.4"], at(1.0));
    let id = file(&mut conn, &[("regression", &["regression"], &[])]).remove(0);
    failing(&conn, id.as_str());
    conn.execute(
        "UPDATE tickets SET needs_os = 'windows' WHERE id = ?1",
        [id.as_str()],
    )
    .unwrap();
    assert_eq!(ask(&mut conn, "m-1", at(1.0)), Value::Null);
    assert_eq!(ask(&mut conn, "m-3", at(2.0))["ticket"], id.as_str());
}

#[test]
fn a_machine_pin_someone_added_still_binds_a_regression() {
    let mut conn = fleet(["1.0", "0.4", "1.0"], at(1.0));
    let id = regression(&mut conn, r#"["regression","machine:multivac"]"#);
    assert_eq!(ask(&mut conn, "m-1", at(1.0)), Value::Null);
    assert_eq!(ask(&mut conn, "m-2", at(2.0))["ticket"], id);
}
