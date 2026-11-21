//! The same disk and queue budget applies both to polling and to lane successors.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_core::ids::TicketId;
use ignite_store::hub;

use super::tests::{at, claim, claim_reserved, file, free, landed, post, register, start};

fn ask(conn: &mut Connection, session: &str, pressure: &DispatchPressure, seconds: f64) -> Value {
    let mut request = free("m-1", &[session]);
    request["pressure"] = json!(pressure);
    request["explain"] = json!(true);
    let (status, reply) = post(conn, "/next", &request, at(seconds));
    assert_eq!(status, 200, "{reply}");
    reply
}

fn classify(conn: &Connection, ticket: &TicketId, cores: f64, body: &str) {
    conn.execute(
        "UPDATE tickets SET cores = ?2, body = ?3 WHERE id = ?1",
        (ticket.as_str(), cores, body),
    )
    .unwrap();
}

#[test]
fn a_pending_reservation_uses_its_warm_tree_and_a_cold_ticket_waits() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[
            ("first", &[], &[]),
            ("second", &[], &[]),
            ("docs", &[], &[]),
        ],
    );
    classify(&conn, &ids[0], 1.0, "Edit `app/src/main.rs`");
    classify(&conn, &ids[1], 1.0, "Edit `app/src/main.rs`");
    classify(&conn, &ids[2], 0.1, "Edit `docs/guide.md`");
    let mut pressure = DispatchPressure {
        free_gb: Some(75),
        outputs: vec!["app/target".to_owned()],
        free_trees: vec![vec!["app/target".to_owned()], vec![]],
        ..DispatchPressure::default()
    };
    let first = ask(&mut conn, "local_a", &pressure, 0.0);
    assert_eq!(first["ticket"], ids[0].as_str());
    assert_eq!(ask(&mut conn, "local_a", &pressure, 1.0), first);
    assert_eq!(
        ask(&mut conn, "local_b", &pressure, 1.0)["ticket"],
        ids[2].as_str()
    );
    let held = ask(&mut conn, "local_c", &pressure, 1.0);
    assert!(
        held["nothing"]
            .as_str()
            .unwrap()
            .contains("12 GB available"),
        "{held}"
    );
    assert_eq!(held["reclaim_to_gb"], 103, "{held}");
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&first, "local_a"),
        at(2.0),
    );
    assert_eq!(status, 200);
    let warm = pressure.free_trees.remove(0);
    pressure.leased.insert(ids[0].clone(), warm);
    pressure.free_gb = Some(103);
    assert_eq!(
        ask(&mut conn, "local_c", &pressure, 2.0)["ticket"],
        ids[1].as_str()
    );
}

#[test]
fn build_handoffs_start_in_threes_then_wait_for_recovery_and_ramp_one_at_a_time() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[
            ("one", &[], &[]),
            ("two", &[], &[]),
            ("three", &[], &[]),
            ("four", &[], &[]),
            ("five", &[], &[]),
        ],
    );
    let mut pressure = DispatchPressure {
        free_gb: Some(500),
        ..DispatchPressure::default()
    };
    for (index, session) in ["local_a", "local_b", "local_c"].into_iter().enumerate() {
        assert_eq!(
            ask(&mut conn, session, &pressure, 0.0)["ticket"],
            ids[index].as_str()
        );
    }
    let paced = ask(&mut conn, "local_d", &pressure, 89.0);
    assert!(
        paced["nothing"].as_str().unwrap().contains("in 1 s"),
        "{paced}"
    );
    pressure.recovery_seconds = 30;
    let recovering = ask(&mut conn, "local_d", &pressure, 90.0);
    assert!(
        recovering["nothing"]
            .as_str()
            .unwrap()
            .contains("healthy build queue"),
        "{recovering}"
    );
    pressure.recovery_seconds = 0;
    assert_eq!(
        ask(&mut conn, "local_d", &pressure, 120.0)["ticket"],
        ids[3].as_str()
    );
    assert!(ask(&mut conn, "local_e", &pressure, 120.0)["nothing"].is_string());
    assert_eq!(
        ask(&mut conn, "local_e", &pressure, 210.0)["ticket"],
        ids[4].as_str()
    );
}

#[test]
fn a_landing_can_close_while_its_lane_successor_waits_for_disk_space() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(
        &mut conn,
        &[("first", &["lane:x"], &[]), ("second", &["lane:x"], &[0])],
    );
    classify(&conn, &ids[1], 1.0, "Edit `app/src/main.rs`");
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim(ids[0].as_str(), "local_a"),
        start(),
    );
    assert_eq!(status, 200);
    let mut pressure = DispatchPressure {
        free_gb: Some(55),
        ..DispatchPressure::default()
    };
    let mut request = landed(ids[0].as_str(), "local_a", "Landed");
    request["pressure"] = json!(pressure);
    let (status, reply) = post(&mut conn, "/landed", &request, at(60.0));
    assert_eq!(status, 200, "{reply}");
    assert!(reply["next"].is_null(), "{reply}");
    let waiting = ask(&mut conn, "local_b", &pressure, 61.0);
    assert!(waiting["nothing"].is_string(), "{waiting}");
    assert_eq!(waiting["reclaim_to_gb"], 90, "{waiting}");

    pressure.free_gb = Some(100);
    assert_eq!(
        ask(&mut conn, "local_b", &pressure, 62.0)["ticket"],
        ids[1].as_str()
    );
}
