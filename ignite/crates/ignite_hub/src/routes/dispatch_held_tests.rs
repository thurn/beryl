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
    let (_, retry) = post(&mut conn, "/next", &free("m-1", &workers), at(1.5));
    assert_eq!(retry, first, "a lost reply can be retried");
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
    // The first worker received its reservation, leaving only the second worker idle.
    asking["idle_workers"].as_array_mut().unwrap().remove(0);
    let (_, second) = post(&mut conn, "/next", &asking, at(2.0));
    assert_eq!(second, Value::Null);
}

#[test]
fn checks_only_takes_a_windows_regression_while_an_active_mac_is_heard_from() {
    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    assert_eq!(
        post(
            &mut conn,
            "/machines/register",
            &json!({
                "machine": "m-win", "name": "windows-host", "os": "windows",
                "cores": 8, "memory_mb": 16000, "families": [], "version": "d97ed0e"
            }),
            start()
        )
        .0,
        200
    );
    let ids = file(
        &mut conn,
        &[
            ("fix the Mac test", &[], &[]),
            ("fix the Windows test", &[], &[]),
        ],
    );
    for (ticket, os) in ids.iter().zip(["macos", "windows"]) {
        regression(&conn, os, ticket, "open");
        conn.execute(
            "UPDATE tickets SET needs_os = ?1 WHERE id = ?2",
            [os, ticket.as_str()],
        )
        .unwrap();
    }
    pause(&mut conn, &json!("m-win"), true);

    let mut windows = free("m-win", &["local_a"]);
    windows["os"] = json!("windows");
    let (_, reply) = post(&mut conn, "/next", &windows, at(1.0));
    assert_eq!(offered(&reply), Some(ids[1].as_str()), "{reply}");
    let (_, mac) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(2.0));
    assert_eq!(offered(&mac), Some(ids[0].as_str()), "{mac}");
}

#[test]
fn checks_only_defers_to_a_recent_active_machine_unless_the_pin_excludes_it() {
    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    pause(&mut conn, &json!("m-2"), true);
    let ids = file(
        &mut conn,
        &[
            ("fix any Mac", &[], &[]),
            ("fix one Mac", &["machine:m-2"], &[]),
        ],
    );
    for (ticket, check) in ids.iter().zip(["portable", "pinned"]) {
        regression(&conn, check, ticket, "open");
    }

    let asking = free("m-2", &["local_a"]);
    let (_, reply) = post(&mut conn, "/next", &asking, at(1.0));
    assert_eq!(offered(&reply), Some(ids[1].as_str()), "{reply}");

    // When the other machine has gone silent, this machine also takes unpinned regressions.
    let (_, later) = post(&mut conn, "/next", &asking, at(1800.0));
    assert_eq!(offered(&later), Some(ids[0].as_str()), "{later}");
}

#[test]
fn fleet_put_turns_off_dispatch_and_the_next_heartbeat_carries_the_policy() {
    use crate::auth::Writer;
    use crate::routes::{Request, fleet};
    use ignite_core::fleet::ProviderSample;
    use ignite_store::hub::provider_usage;

    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    let ids = file(&mut conn, &[("ignite is stuck", &["justiciar"], &[])]);
    let asking = free("m-1", &["local_a"]);
    let (_, existing) = post(&mut conn, "/next", &asking, at(1.0));
    assert_eq!(offered(&existing), Some(ids[0].as_str()));
    provider_usage::record(
        &conn,
        &ProviderSample {
            provider: "codex".to_owned(),
            at: start(),
            machine: "m-1".parse().unwrap(),
            status: "ok".to_owned(),
            label: Some("Weekly".to_owned()),
            percent_used: Some(100.0),
            resets_at: Some(at(600.0).to_string()),
            note: None,
        },
    )
    .unwrap();

    let put = |conn: &mut Connection, version: u64, state: &str| {
        let request = Request {
            method: "PUT".to_owned(),
            path: "/fleet".to_owned(),
            query: Vec::new(),
            headers: Vec::new(),
            writer: Some(Writer {
                machine: "m-1".parse().unwrap(),
                actor: "operator".to_owned(),
            }),
            body: serde_json::to_vec(&json!({
                "version": version,
                "settings": {
                    "version": version,
                    "machines": {"jarvis": {"state": state, "disk_cap_gb": 450}},
                    "providers": {}
                }
            }))
            .unwrap(),
        };
        assert_eq!(fleet::handle(conn, &request, at(2.0)).unwrap().status, 200);
    };
    put(&mut conn, 0, "off");
    let (_, nothing) = post(&mut conn, "/next", &asking, at(3.0));
    assert_eq!(
        nothing,
        Value::Null,
        "an off machine cannot get even its previous reservation"
    );
    let beat = json!({"machine": "m-1", "version": "d97ed0e", "workers": []});
    let (status, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(3.0));
    assert_eq!(status, 200, "{reply}");
    let policy = &reply["fleet"];
    assert_eq!(policy["version"], 1);
    assert_eq!(policy["state"], "off");
    assert_eq!(policy["state_actor"], "operator");
    assert_eq!(policy["disk_cap_gb"], 450);
    assert_eq!(policy["off_platforms"], json!(["macos"]));
    assert_eq!(policy["holds"]["codex"].as_array().unwrap().len(), 1);

    put(&mut conn, 1, "active");
    let (_, resumed) = post(&mut conn, "/next", &asking, at(4.0));
    assert_eq!(resumed, existing);
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(5.0));
    assert_eq!(reply["fleet"]["state"], "active");
    assert_eq!(reply["fleet"]["off_platforms"], json!([]));
}
