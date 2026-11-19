//! What a paused machine is still offered, driven through `POST /next` and `POST /pause`.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::ids::TicketId;
use ignite_store::hub;

use crate::routes::dispatch::tests::{at, file, free, post, start};

fn pause(conn: &mut Connection, machine: &Value, paused: bool) {
    let body = json!({"machine": machine, "paused": paused});
    assert_eq!(post(conn, "/pause", &body, start()).0, 200);
}

/// Marks `ticket` as the ticket of a regression in `state`.
fn regression(conn: &Connection, check: &str, ticket: &TicketId, state: &str) {
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES (?1, ?2, 'b1', 'a1', ?3)",
        [check, ticket.as_str(), state],
    )
    .unwrap();
}

fn offered(reply: &Value) -> Option<&str> {
    reply["ticket"].as_str()
}

#[test]
fn a_paused_machine_gets_the_tickets_holding_master_back_one_at_a_time() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[
            ("feature", &[], &[]),
            ("fix a check", &[], &[]),
            ("fix a check that failed again", &[], &[]),
            ("ignite is stuck", &["justiciar"], &[]),
        ],
    );
    regression(&conn, "rust:a::b", &ids[1], "open");
    regression(&conn, "rust:c::d", &ids[2], "reopened");
    pause(&mut conn, &json!("m-1"), true);
    let workers = ["local_a", "local_b", "local_c"];
    let (_, first) = post(&mut conn, "/next", &free("m-1", &workers), at(1.0));
    assert_eq!(offered(&first), Some(ids[3].as_str()), "{first}");
    // While that reservation holds, the machine gets nothing more, not even a regression.
    let (_, second) = post(&mut conn, "/next", &free("m-1", &workers[1..]), at(2.0));
    assert_eq!(second, Value::Null);
    // Another machine, not paused, still gets the regressions and then the feature.
    let (_, other) = post(&mut conn, "/next", &free("m-2", &["local_d"]), at(3.0));
    assert_eq!(offered(&other), Some(ids[1].as_str()), "{other}");
}

#[test]
fn a_paused_machine_is_offered_a_reopened_regression_but_never_feature_work() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[("feature", &[], &[]), ("fix a check", &[], &[])],
    );
    regression(&conn, "rust:a::b", &ids[1], "reopened");
    pause(&mut conn, &json!("m-1"), true);
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1.0));
    assert_eq!(offered(&first), Some(ids[1].as_str()), "{first}");
    let kind: String = conn
        .query_row("SELECT kind FROM handoffs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kind, "regression");
    conn.execute("UPDATE regressions SET state = 'fixed'", [])
        .unwrap();
    conn.execute("DELETE FROM reservations", []).unwrap();
    let (_, feature) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(2.0));
    assert_eq!(feature, Value::Null);
}

#[test]
fn a_pause_of_every_machine_stops_even_the_tickets_holding_master_back() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("ignite is stuck", &["justiciar"], &[])]);
    pause(&mut conn, &Value::Null, true);
    let (_, nothing) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1.0));
    assert_eq!(nothing, Value::Null);
    let beat = json!({"machine": "m-1", "version": "d97ed0e", "workers": []});
    crate::routes::dispatch::tests::register(&mut conn);
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(2.0));
    assert_eq!(reply["paused_everywhere"], true, "{reply}");
    pause(&mut conn, &Value::Null, false);
    let (_, resumed) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(3.0));
    assert_eq!(offered(&resumed), Some(ids[0].as_str()), "{resumed}");
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(4.0));
    assert!(reply.get("paused_everywhere").is_none(), "{reply}");
}

#[test]
fn a_daemon_paused_on_its_own_asks_only_for_tickets_holding_master_back() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[
            ("feature", &[], &[]),
            ("ignite is stuck", &["justiciar"], &[]),
        ],
    );
    // The hub never heard of this pause; the daemon says it keeps one.
    let mut asking = free("m-1", &["local_a", "local_b"]);
    asking["master_only"] = json!(true);
    let (_, first) = post(&mut conn, "/next", &asking, at(1.0));
    assert_eq!(offered(&first), Some(ids[1].as_str()), "{first}");
    let (_, second) = post(&mut conn, "/next", &asking, at(2.0));
    assert_eq!(second, Value::Null);
}
