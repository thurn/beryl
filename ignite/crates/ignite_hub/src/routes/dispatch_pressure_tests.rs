//! The same build slot and queue limits applies both to polling and to lane successors.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::dispatch_pressure::{BACKLOG_SECONDS, DispatchPressure};
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
fn polling_reserves_an_ordinary_ticket_when_the_queue_is_long_and_the_host_has_room() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("docs", &[], &[])]);
    classify(&conn, &ids[0], 0.1, "Edit `docs/guide.md`");
    let mut pressure = DispatchPressure {
        queue_seconds: BACKLOG_SECONDS + 30,
        recovery_seconds: 90,
        cpu_percent: Some(90),
        ..DispatchPressure::default()
    };
    let waiting = ask(&mut conn, "local_a", &pressure, 0.0);
    assert!(waiting["nothing"].as_str().unwrap().contains("build queue"));

    pressure.cpu_percent = Some(89);
    let reserved = ask(&mut conn, "local_a", &pressure, 1.0);
    assert_eq!(reserved["ticket"], ids[0].as_str());
}

#[test]
fn polling_explains_slot_and_queue_waits_together() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(&mut conn, &[("build", &[], &[])]);
    classify(&conn, &ids[0], 1.0, "Edit `app/src/main.rs`");
    let pressure = DispatchPressure {
        build_slots: 6,
        queue_seconds: 340,
        recovery_seconds: 90,
        cpu_percent: Some(90),
        ..DispatchPressure::default()
    };
    for master_only in [false, true] {
        let mut request = free("m-1", &[]);
        request["room"] = json!({
            "start": 1,
            "providers": [{"name": "claude", "tiers": ["sonnet"]}]
        });
        request["master_only"] = json!(master_only);
        request["pressure"] = json!(pressure);
        request["explain"] = json!(true);
        let (status, reply) = post(&mut conn, "/next", &request, at(0.0));
        assert_eq!(status, 200, "{reply}");
        let reason = reply["nothing"].as_str().unwrap();
        assert!(
            reason.starts_with(&format!("{} waits for a build slot on jarvis", ids[0])),
            "{reason}"
        );
        assert!(
            reason.contains("ticket commands have waited 340 s for cores"),
            "{reason}"
        );
        assert!(reason.contains("90 s of recovery left"), "{reason}");
    }
}

#[test]
fn a_build_ticket_waits_for_a_slot_but_a_light_ticket_can_start() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(&mut conn, &[("build", &[], &[]), ("docs", &[], &[])]);
    classify(&conn, &ids[0], 1.0, "Edit `app/src/main.rs`");
    classify(&conn, &ids[1], 0.1, "Edit `docs/guide.md`");
    let mut pressure = DispatchPressure {
        build_slots: 6,
        free_gb: Some(300),
        ..DispatchPressure::default()
    };
    assert_eq!(
        ask(&mut conn, "local_a", &pressure, 0.0)["ticket"],
        ids[1].as_str()
    );
    let waiting = ask(&mut conn, "local_b", &pressure, 1.0);
    assert!(
        waiting["nothing"]
            .as_str()
            .unwrap()
            .contains("waits for a build slot on jarvis: 6 of 6 held"),
        "{waiting}"
    );

    pressure.build_slots_free = 1;
    assert_eq!(
        ask(&mut conn, "local_b", &pressure, 2.0)["ticket"],
        ids[0].as_str()
    );
}

#[test]
fn a_reservation_counts_until_its_claim_already_holds_a_slot() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("first", &[], &[]), ("second", &[], &[])]);
    for ticket in &ids {
        classify(&conn, ticket, 1.0, "Edit `app/src/main.rs`");
    }
    let mut pressure = DispatchPressure {
        build_slots: 2,
        build_slots_free: 1,
        ..DispatchPressure::default()
    };
    let first = ask(&mut conn, "local_a", &pressure, 0.0);
    assert_eq!(first["ticket"], ids[0].as_str());
    assert_eq!(ask(&mut conn, "local_a", &pressure, 1.0), first);
    let waiting = ask(&mut conn, "local_b", &pressure, 1.0);
    assert!(
        waiting["nothing"].as_str().unwrap().contains("2 of 2 held"),
        "{waiting}"
    );

    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&first, "local_a"),
        at(2.0),
    );
    assert_eq!(status, 200);
    assert!(ask(&mut conn, "local_b", &pressure, 3.0)["nothing"].is_string());

    // The first ticket has taken a slot, and another slot became free in the meantime.
    pressure.slot_holders.insert(ids[0].clone());
    assert_eq!(
        ask(&mut conn, "local_b", &pressure, 4.0)["ticket"],
        ids[1].as_str()
    );
}

#[test]
fn a_daemon_without_slot_fields_can_only_receive_light_work() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("build", &[], &[]), ("docs", &[], &[])]);
    classify(&conn, &ids[0], 1.0, "Edit `app/src/main.rs`");
    classify(&conn, &ids[1], 0.1, "Edit `docs/guide.md`");
    let mut request = free("m-1", &["local_a"]);
    request["pressure"] = json!({
        "free_gb": 300, "recovery_seconds": 0, "queue_seconds": 0,
        "outputs": [], "free_trees": [], "leased": {}
    });
    request["explain"] = json!(true);
    let (status, reservation) = post(&mut conn, "/next", &request, at(0.0));
    assert_eq!(status, 200, "{reservation}");
    assert_eq!(reservation["ticket"], ids[1].as_str());
    request["idle_workers"] = free("m-1", &["local_b"])["idle_workers"].clone();
    let (status, waiting) = post(&mut conn, "/next", &request, at(1.0));
    assert_eq!(status, 200, "{waiting}");
    assert!(
        waiting["nothing"].as_str().unwrap().contains("0 of 0 held"),
        "{waiting}"
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
        build_slots: 6,
        build_slots_free: 6,
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
    let recovering = ask(&mut conn, "local_d", &pressure, 89.0);
    let reason = recovering["nothing"].as_str().unwrap();
    assert!(reason.contains("healthy build queue"), "{reason}");
    assert!(
        reason.contains("next build-ticket handoff in 1 s"),
        "{reason}"
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
fn a_landing_can_close_while_its_lane_successor_waits_for_a_build_slot() {
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
        build_slots: 1,
        ..DispatchPressure::default()
    };
    let mut request = landed(ids[0].as_str(), "local_a", "Landed");
    request["pressure"] = json!(pressure);
    let (status, reply) = post(&mut conn, "/landed", &request, at(60.0));
    assert_eq!(status, 200, "{reply}");
    assert!(reply["next"].is_null(), "{reply}");
    let waiting = ask(&mut conn, "local_b", &pressure, 61.0);
    assert!(waiting["nothing"].is_string(), "{waiting}");
    assert!(
        waiting["nothing"].as_str().unwrap().contains("1 of 1 held"),
        "{waiting}"
    );

    pressure.build_slots_free = 1;
    assert_eq!(
        ask(&mut conn, "local_b", &pressure, 62.0)["ticket"],
        ids[1].as_str()
    );
}

#[test]
fn landing_reserves_the_chain_and_its_brief_carries_the_note() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(
        &mut conn,
        &[("first", &["lane:x"], &[]), ("second", &["lane:x"], &[0])],
    );
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reservation["ticket"], ids[0].as_str());
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(5.0),
    );
    assert_eq!(status, 200);
    let note = "Named counters draw as rows above the plaque.";
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(60.0),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["next"]["ticket"], ids[1].as_str());
    assert_eq!(reply["next"]["session"], "local_a");
    let first = hub::tickets::load(&conn, ids[0].as_str()).unwrap().unwrap();
    assert_eq!(first.close_reason.as_deref(), Some(note));
    assert_eq!(first.landed.unwrap().commits[0].as_str(), "3a9e4c1");
    let (status, retry) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(60.5),
    );
    assert_eq!(status, 200);
    assert_eq!(
        retry, reply,
        "a lost reply must keep the same code and expiry"
    );
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reply["next"], "local_a"),
        at(61.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["needs"][0]["ticket"], ids[0].as_str());
    assert_eq!(brief["needs"][0]["landed"]["note"], note);
    // The claim consumed the reservation, so a replay cannot hand it off again.
    let (status, retry) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(62.0),
    );
    assert_eq!(status, 200);
    assert!(retry["next"].is_null());
}
