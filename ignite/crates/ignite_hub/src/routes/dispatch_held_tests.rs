//! What a paused machine is still offered, driven through `POST /next` and `POST /pause`.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::config::Os;
use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::{
    IncompleteReason, IncompleteReport, TipState, TipVerified, TipWaiting,
};
use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;
use ignite_store::hub;

use crate::routes::dispatch::tests::{at, file, free, post, start};
use crate::upkeep::{filing::UPKEEP, justiciar, stuck};

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

fn register_windows(conn: &mut Connection, id: &str, name: &str) {
    let register = json!({
        "machine": id, "name": name, "os": "windows", "cores": 8,
        "memory_mb": 16000, "families": [], "version": "d97ed0e"
    });
    assert_eq!(post(conn, "/machines/register", &register, start()).0, 200);
}

fn windows_incomplete(commit: &str, check: &str, holds_tip: bool) -> IncompleteReport {
    IncompleteReport {
        check: check.parse().unwrap(),
        machine: "m-win".parse().unwrap(),
        host: "windows-host".to_owned(),
        platform: Some(Os::Windows),
        attempts: 2,
        reason: IncompleteReason::Hung,
        commit: commit.parse().unwrap(),
        output: format!("{check} hung"),
        checks: Vec::new(),
        count: 1,
        product: None,
        holds_tip,
    }
}

fn approve(conn: &mut Connection, ticket: &str, now: Timestamp) {
    use ignite_core::protocol::tickets::{BatchRequest, Operation, TicketFields};
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Update {
            id: ticket.to_owned(),
            set: Some(TicketFields {
                status: Some(Status::Open),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::new(),
            unset_meta: Vec::new(),
        }],
    };
    assert!(matches!(
        hub::batch::apply(conn, &request, "operator", "m-1", now).unwrap(),
        hub::batch::BatchOutcome::Applied(_)
    ));
}

#[test]
fn checks_only_takes_one_tip_repair_with_no_worker_room() {
    let mut conn = hub::open_in_memory().unwrap();
    register_windows(&mut conn, "m-win", "windows-host");
    register_windows(&mut conn, "m-other", "other-host");
    pause(&mut conn, &json!("m-win"), true);
    let ordinary = stuck::incomplete(
        &mut conn,
        UPKEEP,
        &windows_incomplete("aa11", "rust:ordinary", false),
        at(1.0),
    )
    .unwrap()
    .ticket;
    approve(&mut conn, &ordinary, at(2.0));
    let mut ask = free("m-win", &[]);
    ask["os"] = json!("windows");
    ask["free_cores"] = json!(0);
    ask["room"] = json!({"start": 0, "providers": [{"name": "claude", "tiers": ["sonnet"]}]});
    assert_eq!(post(&mut conn, "/next", &ask, at(3.0)).1, Value::Null);

    let first = stuck::incomplete(
        &mut conn,
        UPKEEP,
        &windows_incomplete("abcd", "build:first", true),
        at(4.0),
    )
    .unwrap()
    .ticket;
    assert_ne!(first, ordinary);
    let second = stuck::incomplete(
        &mut conn,
        UPKEEP,
        &windows_incomplete("abcd", "native:second", true),
        at(5.0),
    )
    .unwrap()
    .ticket;
    let mut wrong_host = ask.clone();
    wrong_host["machine"] = json!("m-other");
    assert_eq!(
        post(&mut conn, "/next", &wrong_host, at(6.0)).1,
        Value::Null
    );
    let (_, reservation) = post(&mut conn, "/next", &ask, at(7.0));
    assert_eq!(offered(&reservation), Some(first.as_str()), "{reservation}");
    assert_eq!(reservation["master_blocker"], true);
    assert!(
        hub::fleet::reservation_for_worker(
            &conn,
            "m-win",
            reservation["session"].as_str().unwrap()
        )
        .unwrap()
        .unwrap()
        .master_blocker
    );
    assert!(
        hub::fleet::reservation_of(&conn, &second)
            .unwrap()
            .is_none()
    );
    assert_eq!(post(&mut conn, "/next", &ask, at(8.0)).1, Value::Null);

    hub::tips::verified(
        &conn,
        &TipVerified {
            commit: "dcba".parse().unwrap(),
            machine: "m-win".parse().unwrap(),
            state: TipState::Verified,
            regression_passes: None,
            failure: None,
        },
        at(9.0),
    )
    .unwrap();
    assert_eq!(post(&mut conn, "/next", &ask, at(10.0)).1, Value::Null);

    let wait = TipWaiting {
        commit: "ef01".parse().unwrap(),
        machine: "m-other".parse().unwrap(),
        platform: Os::Windows,
        checks: vec!["rust:ordinary".parse().unwrap()],
        waited_s: 3_600.0,
        unrunnable: Vec::new(),
    };
    let filed = stuck::tip_waiting(&mut conn, UPKEEP, &wait, at(11.0)).unwrap();
    assert_eq!(filed.len(), 2);
    ask["explain"] = json!(true);
    let (_, repair) = post(&mut conn, "/next", &ask, at(12.0));
    assert_eq!(offered(&repair), Some(ordinary.as_str()), "{repair}");
    assert_eq!(repair["master_blocker"], true);
    let mut other = free("m-other", &["local_b"]);
    other["os"] = json!("windows");
    pause(&mut conn, &json!("m-other"), true);
    let (_, platform_repair) = post(&mut conn, "/next", &other, at(13.0));
    assert!(
        filed
            .iter()
            .any(|result| Some(result.ticket.as_str()) == offered(&platform_repair))
    );
    assert_eq!(platform_repair["master_blocker"], true);
}

#[test]
fn retrying_a_reservation_reflects_a_later_blocking_sighting_of_the_same_cause() {
    let mut conn = hub::open_in_memory().unwrap();
    register_windows(&mut conn, "m-win", "windows-host");
    let mut report = windows_incomplete("abcd", "rust:paths", false);
    let filed = stuck::incomplete(&mut conn, UPKEEP, &report, at(1.0)).unwrap();
    approve(&mut conn, &filed.ticket, at(2.0));
    let mut ask = free("m-win", &["local_a"]);
    ask["os"] = json!("windows");
    let (_, ordinary) = post(&mut conn, "/next", &ask, at(3.0));
    assert_eq!(
        offered(&ordinary),
        Some(filed.ticket.as_str()),
        "{ordinary}"
    );
    assert_ne!(ordinary["master_blocker"], true);
    report.holds_tip = true;
    let promoted = stuck::incomplete(&mut conn, UPKEEP, &report, at(4.0)).unwrap();
    assert_eq!(promoted.ticket, filed.ticket);
    pause(&mut conn, &json!("m-win"), true);
    let (_, retry) = post(&mut conn, "/next", &ask, at(5.0));
    assert_eq!(offered(&retry), Some(filed.ticket.as_str()), "{retry}");
    assert_eq!(retry["code"], ordinary["code"]);
    assert_eq!(retry["master_blocker"], true);
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
fn checks_only_offers_the_native_repair_before_its_unclaimed_escalation() {
    let (mut conn, repair, _, now) = stalled_repair(&["machine:jarvis"]);
    let mut asking = free("m-1", &[]);
    asking["room"] = json!({"start": 0, "providers": [{"name": "claude", "tiers": ["opus"]}]});
    let (_, reply) = post(&mut conn, "/next", &asking, now);
    assert_eq!(offered(&reply), Some(repair.as_str()), "{reply}");
    assert_eq!(reply["master_blocker"], true);
    let (_, second) = post(&mut conn, "/next", &asking, now);
    assert_eq!(second, Value::Null);
}

#[test]
fn checks_only_offers_the_escalation_when_its_repair_lacks_a_provider() {
    let (mut conn, _, escalation, now) = stalled_repair(&["machine:jarvis", "provider:codex"]);
    let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), now);
    assert_eq!(offered(&reply), Some(escalation.as_str()), "{reply}");
}

fn stalled_repair(labels: &[&str]) -> (Connection, TicketId, String, Timestamp) {
    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    pause(&mut conn, &json!("m-1"), true);
    let ids = file(
        &mut conn,
        &[
            ("landed work", &[], &[]),
            ("repair native check", labels, &[]),
        ],
    );
    let repair = ids[1].clone();
    conn.execute(
        "UPDATE tickets SET landed_at = ?1 WHERE id = ?2",
        [start().to_string(), ids[0].to_string()],
    )
    .unwrap();
    conn.execute(
        "UPDATE tickets SET needs_os = 'macos' WHERE id = ?1",
        [repair.as_str()],
    )
    .unwrap();
    regression(&conn, "native:build", &repair, "open");
    let now = at(justiciar::MASTER_LAG_SECONDS + 1.0);
    let filed = justiciar::master_lag(&mut conn, now).unwrap();
    assert_eq!(filed.len(), 1);
    let escalation = filed[0].ticket.clone();
    assert_ne!(escalation, repair.as_str());
    (conn, repair, escalation, now)
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
    register_windows(&mut conn, "m-win", "windows-host");
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
    let mut wrong_os = windows.clone();
    wrong_os["os"] = json!("macos");
    assert_eq!(post(&mut conn, "/next", &wrong_os, at(0.5)).0, 400);
    let (_, reply) = post(&mut conn, "/next", &windows, at(1.0));
    assert_eq!(offered(&reply), Some(ids[1].as_str()), "{reply}");
    let (_, mac) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(2.0));
    assert_eq!(offered(&mac), Some(ids[0].as_str()), "{mac}");
}

#[test]
fn checks_only_defers_to_a_recent_active_machine_unless_the_pin_excludes_it() {
    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    let beat = json!({"machine": "m-1", "version": "v", "workers": [], "cli_apps": ["claude"]});
    assert_eq!(
        post(&mut conn, "/machines/heartbeat", &beat, start()).0,
        200
    );
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
fn checks_only_takes_a_blocker_when_the_active_machines_model_is_held() {
    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    pause(&mut conn, &json!("m-2"), true);
    let ticket = file(
        &mut conn,
        &[("repair", &["provider:claude", "tier:sonnet"], &[])],
    )
    .remove(0);
    regression(&conn, "build:app", &ticket, "open");

    let mut active = crate::routes::dispatch::placement_tests::capacity(false);
    active.providers[0]
        .model_ids
        .insert("sonnet".into(), "claude-opus".into());
    crate::routes::dispatch::placement_tests::record(&conn, "m-1", &active, start());
    let checking = free("m-2", &["local_a"]);
    let (_, first) = post(&mut conn, "/next", &checking, at(1.0));
    assert_eq!(first, Value::Null);

    hub::provider_usage::record(
        &conn,
        &ignite_core::fleet::ProviderSample {
            provider: "claude".into(),
            at: start(),
            machine: "m-1".parse().unwrap(),
            status: "ok".into(),
            label: Some("Weekly · Opus".into()),
            percent_used: Some(99.0),
            resets_at: Some(at(60.0).to_string()),
            note: None,
        },
    )
    .unwrap();
    // The active machine maps this ticket's tier to Opus. The checks-only machine uses Sonnet.
    let (_, second) = post(&mut conn, "/next", &checking, at(2.0));
    assert_eq!(offered(&second), Some(ticket.as_str()), "{second}");
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
                    "machines": {"jarvis": {"state": state, "tree_space_gb": 450}},
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
    assert_eq!(policy["tree_space_gb"], 450);
    assert_eq!(policy["off_platforms"], json!(["macos"]));
    assert_eq!(policy["holds"]["codex"].as_array().unwrap().len(), 1);

    put(&mut conn, 1, "active");
    let (_, resumed) = post(&mut conn, "/next", &asking, at(4.0));
    assert_eq!(resumed, existing);
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(5.0));
    assert_eq!(reply["fleet"]["state"], "active");
    assert_eq!(reply["fleet"]["off_platforms"], json!([]));
}

#[test]
fn only_platforms_without_an_online_active_machine_are_deferred() {
    use ignite_core::fleet::MachineState;
    use ignite_store::hub::fleet_settings;

    let mut conn = hub::open_in_memory().unwrap();
    crate::routes::dispatch::tests::register(&mut conn);
    for (machine, name) in [("m-2", "cygnus"), ("m-3", "spare")] {
        let register = json!({"machine": machine, "name": name, "os": "windows", "cores": 4,
                              "memory_mb": 4096, "families": [], "version": "d97ed0e"});
        assert_eq!(
            post(&mut conn, "/machines/register", &register, start()).0,
            200
        );
    }
    fleet_settings::set_machine_state(
        &mut conn,
        "m-2",
        MachineState::ChecksOnly,
        "operator",
        at(1.0),
    )
    .unwrap();
    let windows = json!({"machine": "m-2", "version": "d97ed0e", "workers": []});
    let mac = json!({"machine": "m-1", "version": "d97ed0e", "workers": []});
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &mac, at(2.0));
    assert!(reply["fleet"].get("checks_only_platforms").is_none());
    assert_eq!(
        post(&mut conn, "/machines/heartbeat", &windows, at(1_800.0)).0,
        200
    );
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &mac, at(1_801.0));
    assert_eq!(reply["fleet"]["checks_only_platforms"], json!(["windows"]));
    assert_eq!(reply["fleet"]["off_platforms"], json!([]));

    let active = json!({"machine": "m-3", "version": "d97ed0e", "workers": []});
    assert_eq!(
        post(&mut conn, "/machines/heartbeat", &active, at(1_802.0)).0,
        200
    );
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &mac, at(1_803.0));
    assert!(reply["fleet"].get("checks_only_platforms").is_none());
}
