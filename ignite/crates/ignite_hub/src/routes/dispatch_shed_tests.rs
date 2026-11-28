//! What a heartbeat's reply says about the machine's build tickets, and how an order to shed
//! and a finished shed reach the hub (see `ignite_core::protocol::hub::shed`).

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_store::hub::fleet_settings;

use super::tests::{at, claim, file, open, post, register};

/// A heartbeat with a sample from a machine with 6 shared cores, a limit of 2 build tickets.
fn sampled_beat() -> Value {
    let pressure = DispatchPressure {
        build_slots: 6,
        build_slots_free: 6,
        ..DispatchPressure::default()
    };
    json!({"machine": "m-1", "version": "d97ed0e", "workers": [], "sample": {
        "pressure": pressure, "reclaimable": null, "cpu_percent": null, "queued_cores": 0,
        "swap_used_mb": null, "idle_workers": 0, "room": {"start": 0, "providers": []},
        "free_ticket_trees": 0, "admit_cores": 6, "draining": false, "shared_failure": null,
    }})
}

fn beat(conn: &mut Connection, body: &Value, seconds: f64) -> Value {
    let (status, reply) = post(conn, "/machines/heartbeat", body, at(seconds));
    assert_eq!(status, 200, "{reply}");
    reply
}

#[test]
fn a_sampled_heartbeat_reports_the_build_tickets_held_newest_first_against_the_limit() {
    let mut conn = open();
    register(&mut conn);
    let ids = file(
        &mut conn,
        &[
            ("one", &[], &[]),
            ("two", &[], &[]),
            ("fix", &["justiciar"], &[]),
        ],
    );
    for (n, (id, session)) in ids
        .iter()
        .zip(["local_a", "local_b", "local_c"])
        .enumerate()
    {
        let (status, reply) = post(
            &mut conn,
            "/claims",
            &claim(id.as_str(), session),
            at(n as f64),
        );
        assert_eq!(status, 200, "{reply}");
    }

    let reply = beat(&mut conn, &sampled_beat(), 10.0);

    let held = &reply["build_tickets"];
    assert_eq!(held["limit"], 2, "{reply}");
    let tickets: Vec<&str> = held["tickets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|held| held["ticket"].as_str().unwrap())
        .collect();
    assert_eq!(tickets, [ids[2].as_str(), ids[1].as_str(), ids[0].as_str()]);
    assert_eq!(held["tickets"][0]["master_blocker"], true);
    assert_eq!(held["tickets"][1]["session"], "local_b");
    let plain = json!({"machine": "m-1", "version": "d97ed0e", "workers": []});
    assert!(beat(&mut conn, &plain, 11.0).get("build_tickets").is_none());
}

#[test]
fn an_order_rides_the_next_sampled_heartbeat_once_and_a_shed_is_a_fleet_change() {
    let mut conn = open();
    register(&mut conn);
    let order = json!({"machine": "jarvis", "to": 1, "by": "cli-claude-incident"});
    assert_eq!(post(&mut conn, "/shed", &order, at(1.0)).0, 200);
    let unknown = json!({"machine": "nowhere", "by": "the operator"});
    assert_eq!(post(&mut conn, "/shed", &unknown, at(1.0)).0, 404);

    let plain = json!({"machine": "m-1", "version": "d97ed0e", "workers": []});
    assert!(
        beat(&mut conn, &plain, 2.0).get("shed").is_none(),
        "only with a sample"
    );
    assert_eq!(beat(&mut conn, &sampled_beat(), 3.0)["shed"], order);
    assert!(
        beat(&mut conn, &sampled_beat(), 4.0).get("shed").is_none(),
        "delivered once"
    );

    let done = json!({"machine": "m-1", "summary": "shed T1: held 3 of limit 2 build tickets"});
    assert_eq!(post(&mut conn, "/shed/done", &done, at(5.0)).0, 200);
    let changes = fleet_settings::changes(&conn, 1).unwrap();
    assert_eq!(changes[0].actor, "jarvis");
    assert_eq!(
        changes[0].summary,
        "shed T1: held 3 of limit 2 build tickets"
    );
}
