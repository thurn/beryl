//! Build limits apply both to polling and to lane successors.

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::dispatch_pressure::{BACKLOG_SECONDS, DispatchPressure};
use ignite_core::ids::TicketId;
use ignite_core::manifest::Manifest;
use ignite_store::hub;

use super::tests::{
    at, claim, claim_reserved, file, free, landed, open, post, register, release, start,
};

fn ask(conn: &mut Connection, session: &str, pressure: &DispatchPressure, seconds: f64) -> Value {
    ask_with_cores(conn, session, pressure, seconds, None)
}

fn ask_with_cores(
    conn: &mut Connection,
    session: &str,
    pressure: &DispatchPressure,
    seconds: f64,
    admit_cores: Option<f64>,
) -> Value {
    let mut request = free("m-1", &[session]);
    request["pressure"] = json!(pressure);
    if let Some(cores) = admit_cores {
        request["admit_cores"] = json!(cores);
    }
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
    let mut conn = open();
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
fn a_bug_that_names_only_documents_still_waits_for_a_build_slot() {
    // A bug report names the report and the guide to read, never the files its fix will
    // change, so those names must not let it start as light work.
    let mut conn = open();
    let ids = file(&mut conn, &[("bug", &[], &[]), ("docs", &[], &[])]);
    let body = "Read `reports/one/report.md` first, as `docs/guide.md` says.";
    for id in &ids {
        classify(&conn, id, 0.1, body);
    }
    conn.execute(
        "UPDATE tickets SET type = 'bug' WHERE id = ?1",
        [ids[0].as_str()],
    )
    .unwrap();
    let pressure = DispatchPressure {
        build_slots: 6,
        build_slots_free: 0,
        ..DispatchPressure::default()
    };
    let reserved = ask(&mut conn, "local_a", &pressure, 0.0);
    assert_eq!(reserved["ticket"], ids[1].as_str(), "{reserved}");
    let waiting = ask(&mut conn, "local_b", &pressure, 1.0);
    let reason = waiting["nothing"].as_str().unwrap();
    assert!(reason.starts_with(ids[0].as_str()), "{waiting}");
    assert!(reason.contains("waits for a build slot"), "{waiting}");
}

#[test]
fn polling_explains_slot_and_queue_waits_together() {
    let mut conn = open();
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
    let mut conn = open();
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
        waiting["nothing"].as_str().unwrap().contains(
            "waits for a build slot on jarvis: no slot available to tickets (6 configured)"
        ),
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
    let mut conn = open();
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
        waiting["nothing"]
            .as_str()
            .unwrap()
            .contains("(2 configured)"),
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
    let mut conn = open();
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
        waiting["nothing"]
            .as_str()
            .unwrap()
            .contains("(0 configured)"),
        "{waiting}"
    );
}

#[test]
fn build_handoffs_start_in_threes_then_wait_for_recovery_and_ramp_one_at_a_time() {
    for admit_cores in [None, Some(15.0)] {
        check_ramp(admit_cores);
    }
}

fn check_ramp(admit_cores: Option<f64>) {
    let mut conn = open();
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
    for id in &ids {
        classify(&conn, id, 0.1, "Investigate a reported bug");
    }
    let mut pressure = DispatchPressure {
        build_slots: 6,
        build_slots_free: 6,
        ..DispatchPressure::default()
    };
    for (index, session) in ["local_a", "local_b", "local_c"].into_iter().enumerate() {
        assert_eq!(
            ask_with_cores(&mut conn, session, &pressure, 0.0, admit_cores)["ticket"],
            ids[index].as_str()
        );
    }
    let paced = ask_with_cores(&mut conn, "local_d", &pressure, 149.0, admit_cores);
    assert!(
        paced["nothing"].as_str().unwrap().contains("in 1 s"),
        "{paced}"
    );
    pressure.recovery_seconds = 30;
    let recovering = ask_with_cores(&mut conn, "local_d", &pressure, 149.0, admit_cores);
    let reason = recovering["nothing"].as_str().unwrap();
    assert!(reason.contains("healthy build queue"), "{reason}");
    assert!(
        reason.contains("next build-ticket handoff in 1 s"),
        "{reason}"
    );
    pressure.recovery_seconds = 0;
    assert_eq!(
        ask_with_cores(&mut conn, "local_d", &pressure, 150.0, admit_cores)["ticket"],
        ids[3].as_str()
    );
    assert!(
        ask_with_cores(&mut conn, "local_e", &pressure, 150.0, admit_cores)["nothing"].is_string()
    );
    assert_eq!(
        ask_with_cores(&mut conn, "local_e", &pressure, 300.0, admit_cores)["ticket"],
        ids[4].as_str()
    );
}

#[test]
fn six_shared_cores_admit_two_builds_even_after_the_ramp_wait() {
    let mut conn = open();
    let ids = file(
        &mut conn,
        &[
            ("one", &[], &[]),
            ("two", &[], &[]),
            ("three", &[], &[]),
            ("docs", &[], &[]),
        ],
    );
    for id in &ids[..3] {
        classify(&conn, id, 0.25, "Edit `app/src/main.rs`");
    }
    classify(&conn, &ids[3], 0.1, "Edit `docs/guide.md`");
    let mut pressure = DispatchPressure {
        build_slots: 9,
        build_slots_free: 9,
        ..DispatchPressure::default()
    };
    let ask = |conn: &mut Connection, session: &str, pressure: &DispatchPressure, time| {
        ask_with_cores(conn, session, pressure, time, Some(6.0))
    };
    let first = ask(&mut conn, "local_a", &pressure, 0.0);
    assert_eq!(first["ticket"], ids[0].as_str());
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&first, "local_a"),
            at(1.0)
        )
        .0,
        200
    );
    pressure.slot_holders.insert(ids[0].clone());
    pressure.build_slots_free -= 1;
    assert_eq!(
        ask(&mut conn, "local_b", &pressure, 2.0)["ticket"],
        ids[1].as_str()
    );
    assert_eq!(
        ask(&mut conn, "local_d", &pressure, 120.0)["ticket"],
        ids[3].as_str()
    );
    let waiting = ask(&mut conn, "local_e", &pressure, 120.0);
    assert!(
        waiting["nothing"]
            .as_str()
            .unwrap()
            .contains("2 build tickets held, limit 2"),
        "{waiting}"
    );
    let mut gave_back = release(ids[0].as_str(), "local_a");
    gave_back["needs"] = json!("person");
    assert_eq!(post(&mut conn, "/release", &gave_back, at(121.0)).0, 200);
    assert_eq!(
        ask(&mut conn, "local_e", &pressure, 122.0)["ticket"],
        ids[2].as_str()
    );
}

fn publish_build_checks(conn: &Connection) {
    let source = r#"
[family.replay]
kind = "regression"
adapter = "jsonl"
platforms = "any"
needs = ["inspector"]
list = "list"
run = "run"
inputs = ["data/replays/*.json"]

[family.source]
kind = "regression"
adapter = "jsonl"
platforms = "any"
list_needs = ["inspector"]
list = "list"
run = "run"
inputs = ["tools/audit.py"]

[family.notes]
kind = "regression"
adapter = "jsonl"
platforms = "any"
needs = ["scripts"]
list = "list"
run = "run"
inputs = ["docs/**"]

[product.inspector]
build = "compile"
compiles = true
inputs = ["config/inspector.json"]
outputs = ["app/output"]

[product.scripts]
build = "install"
inputs = ["tools/lock.json"]
outputs = ["tools/environment"]
"#;
    let manifest =
        Manifest::from_files(&[(PathBuf::from("checks.toml"), source.to_owned())]).unwrap();
    super::publish_check_families(conn, &manifest).unwrap();
}

#[test]
fn a_build_reservation_from_polling_limits_a_lane_successor_and_vice_versa() {
    for lane_first in [false, true] {
        let mut conn = open();
        register(&mut conn);
        publish_build_checks(&conn);
        let [rust, config, predecessor, polling, lane, light] = file(
            &mut conn,
            &[
                ("Rust", &[], &[]),
                ("Inspector config", &[], &[]),
                ("Lane predecessor", &["lane:checks"], &[]),
                ("Replay update", &[], &[]),
                ("Source audit", &["lane:checks"], &[2]),
                ("Docs", &[], &[]),
            ],
        )
        .try_into()
        .unwrap();
        for (ticket, body) in [
            (&rust, "Edit `app/src/{one.rs, nested/two.rs}`"),
            (&config, "Edit `config/inspector.json`"),
            (&predecessor, "Edit `docs/lane.md`"),
            (&polling, "Edit `data/replays/fixture.json`"),
            (&lane, "Edit `tools/audit.py`"),
            (&light, "Edit `docs/guide.md`"),
        ] {
            classify(&conn, ticket, 0.1, body);
        }
        for (ticket, session, tree) in [
            (&rust, "local_rust", "tree-01"),
            (&config, "local_config", "tree-02"),
            (&predecessor, "local_lane", "tree-03"),
        ] {
            let mut request = claim(ticket.as_str(), session);
            request["tree"] = json!(tree);
            let (status, reply) = post(&mut conn, "/claims", &request, start());
            assert_eq!(status, 200, "{reply}");
        }

        let pressure = DispatchPressure {
            shared_cores: Some(9),
            build_slots: 6,
            build_slots_free: 5,
            slot_holders: [rust].into(),
            ..DispatchPressure::default()
        };
        let mut finish = landed(predecessor.as_str(), "local_lane", "Landed");
        finish["pressure"] = json!(pressure);
        if lane_first {
            let (status, reply) = post(&mut conn, "/landed", &finish, at(1.0));
            assert_eq!(status, 200, "{reply}");
            assert_eq!(reply["next"]["ticket"], lane.as_str());
            assert_eq!(
                ask(&mut conn, "local_poll", &pressure, 2.0)["ticket"],
                light.as_str()
            );
        } else {
            assert_eq!(
                ask(&mut conn, "local_poll", &pressure, 1.0)["ticket"],
                polling.as_str()
            );
            let (status, reply) = post(&mut conn, "/landed", &finish, at(2.0));
            assert_eq!(status, 200, "{reply}");
            assert!(reply["next"].is_null(), "{reply}");
            assert_eq!(
                ask(&mut conn, "local_light", &pressure, 3.0)["ticket"],
                light.as_str()
            );
        }
        let waiting = ask(&mut conn, "local_next", &pressure, 120.0);
        let blocked = if lane_first { polling } else { lane };
        let reason = waiting["nothing"].as_str().unwrap();
        assert!(reason.starts_with(blocked.as_str()), "{waiting}");
        assert!(
            reason.contains("3 build tickets held, limit 3"),
            "{waiting}"
        );
    }
}

#[test]
fn a_light_claim_that_acquires_a_build_slot_counts_against_the_core_limit() {
    let mut conn = open();
    register(&mut conn);
    let ids = file(&mut conn, &[("tool", &[], &[]), ("build", &[], &[])]);
    classify(&conn, &ids[0], 0.1, "Edit `tools/verify.py`");
    classify(&conn, &ids[1], 1.0, "Edit `app/src/main.rs`");
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim(ids[0].as_str(), "local_a"),
            start()
        )
        .0,
        200
    );
    let pressure = DispatchPressure {
        build_slots: 9,
        build_slots_free: 8,
        slot_holders: [ids[0].clone()].into(),
        ..DispatchPressure::default()
    };
    let wait = ask_with_cores(&mut conn, "local_b", &pressure, 120.0, Some(2.0));
    assert!(
        wait["nothing"]
            .as_str()
            .unwrap()
            .contains("1 build tickets held, limit 1"),
        "{wait}"
    );
    let mut gave_back = release(ids[0].as_str(), "local_a");
    gave_back["needs"] = json!("person");
    assert_eq!(post(&mut conn, "/release", &gave_back, at(121.0)).0, 200);
    // A heartbeat can still list the previous holder immediately after release.
    assert_eq!(
        ask_with_cores(&mut conn, "local_b", &pressure, 122.0, Some(2.0))["ticket"],
        ids[1].as_str()
    );
}

#[test]
fn a_lane_successor_can_use_the_build_capacity_its_predecessor_releases() {
    for first_uses_build_capacity in [false, true] {
        let mut conn = open();
        register(&mut conn);
        let ids = file(
            &mut conn,
            &[
                ("first", &["lane:x"], &[]),
                ("second", &["lane:x"], &[0]),
                ("one", &[], &[]),
                ("two", &[], &[]),
                ("three", &[], &[]),
            ],
        );
        for id in &ids {
            classify(&conn, id, 1.0, "Edit `app/src/main.rs`");
        }
        if !first_uses_build_capacity {
            classify(&conn, &ids[0], 0.1, "Edit `docs/guide.md`");
        }
        let session = "local_a";
        assert_eq!(
            post(
                &mut conn,
                "/claims",
                &claim(ids[0].as_str(), session),
                start()
            )
            .0,
            200
        );
        let pressure = DispatchPressure {
            shared_cores: Some(9),
            build_slots: 9,
            build_slots_free: 9,
            ..DispatchPressure::default()
        };
        let other_builds = if first_uses_build_capacity { 2 } else { 3 };
        for (id, session) in ids
            .iter()
            .skip(2)
            .take(other_builds)
            .zip(["local_b", "local_c", "local_d"])
        {
            assert_eq!(
                ask(&mut conn, session, &pressure, 0.0)["ticket"],
                id.as_str()
            );
        }
        let mut request = landed(ids[0].as_str(), session, "Landed");
        request["pressure"] = json!(pressure);
        let (status, reply) = post(&mut conn, "/landed", &request, at(120.0));
        assert_eq!(status, 200, "{reply}");
        if first_uses_build_capacity {
            assert_eq!(reply["next"]["ticket"], ids[1].as_str());
        } else {
            assert!(reply["next"].is_null(), "{reply}");
            let waiting = ask(&mut conn, "local_e", &pressure, 121.0);
            assert!(
                waiting["nothing"]
                    .as_str()
                    .unwrap()
                    .contains("3 build tickets held, limit 3"),
                "{waiting}"
            );
        }
    }
}

#[test]
fn a_landing_can_close_while_its_lane_successor_waits_for_a_build_slot() {
    let mut conn = open();
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
        waiting["nothing"]
            .as_str()
            .unwrap()
            .contains("(1 configured)"),
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
    let mut conn = open();
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
