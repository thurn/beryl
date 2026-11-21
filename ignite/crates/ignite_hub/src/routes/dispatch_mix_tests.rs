//! The room-based hand-off protocol, exercised against a migrated hub database.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::fleet::ProviderSample;
use ignite_core::time::Timestamp;
use ignite_store::hub::{fleet_settings, open_in_memory, provider_usage};

use super::Request;
use super::tests::{at, claim_reserved, file, free, landed, post, release, start};
use crate::auth::Writer;
use crate::routes::fleet;

fn set_mix(conn: &mut Connection, shares: [u32; 3], third_enabled: bool) {
    let version = fleet_settings::settings(conn).unwrap().version;
    let [claude, codex, third] = shares;
    let request = Request {
        method: "PUT".into(),
        path: "/fleet".into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: "m-1".parse().unwrap(),
            actor: "operator".into(),
        }),
        body: serde_json::to_vec(&json!({
            "version": version,
            "settings": { "version": version, "machines": {}, "providers": {
                "claude": {"enabled": true, "share": claude, "stop_percent": 99},
                "codex": {"enabled": true, "share": codex, "stop_percent": 99},
                "third": {"enabled": third_enabled, "share": third, "stop_percent": 99}
            }}
        }))
        .unwrap(),
        headers: Vec::new(),
    };
    let reply = fleet::handle(conn, &request, start()).unwrap();
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
}

fn machine_room(machine: &str, start: u32, providers: &[&str]) -> Value {
    let mut ask = free(machine, &[]);
    ask.as_object_mut().unwrap().remove("idle_workers");
    ask["explain"] = json!(true);
    ask["room"] = json!({
        "start": start,
        "providers": providers.iter().map(|name| json!({
            "name": name, "tiers": ["opus", "sonnet"]
        })).collect::<Vec<_>>()
    });
    ask
}

fn ask(conn: &mut Connection, machine: &Value, now: Timestamp) -> Value {
    let (status, reply) = post(conn, "/next", machine, now);
    assert_eq!(status, 200, "{reply}");
    reply
}

fn hold_claude(conn: &Connection, label: &str, reset: Timestamp) {
    provider_usage::record(
        conn,
        &ProviderSample {
            provider: "claude".to_owned(),
            at: start(),
            machine: "m-2".parse().unwrap(),
            status: "ok".into(),
            label: Some(label.to_owned()),
            percent_used: Some(99.0),
            resets_at: Some(reset.to_string()),
            note: None,
        },
    )
    .unwrap();
}

fn claim_provider(conn: &Connection, ticket: &str) -> Option<String> {
    conn.query_row(
        "SELECT provider FROM tickets WHERE id = ?1",
        [ticket],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn equal_shares_alternate_across_machines_counting_claims_and_reservations() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [50, 50, 0], true);
    file(
        &mut conn,
        &[
            ("one", &[], &[]),
            ("two", &[], &[]),
            ("three", &[], &[]),
            ("four", &[], &[]),
        ],
    );
    let providers = ["third", "codex", "claude"];
    let first_machine = machine_room("m-1", 2, &providers);
    let second_machine = machine_room("m-2", 2, &providers);
    let first = ask(&mut conn, &first_machine, start());
    assert_eq!(first["provider"], "claude");
    let claimed = claim_reserved(&first, "cli-claude-ea631acc");
    assert_eq!(post(&mut conn, "/claims", &claimed, at(1.0)).0, 200);
    let ticket = first["ticket"].as_str().unwrap();
    assert_eq!(claim_provider(&conn, ticket).as_deref(), Some("claude"));
    assert_eq!(
        ask(&mut conn, &second_machine, at(2.0))["provider"],
        "codex"
    );
    assert_eq!(
        ask(&mut conn, &second_machine, at(3.0))["provider"],
        "claude"
    );
    assert_eq!(ask(&mut conn, &first_machine, at(4.0))["provider"], "codex");
    assert_eq!(
        post(
            &mut conn,
            "/release",
            &release(ticket, "cli-claude-ea631acc"),
            at(5.0)
        )
        .0,
        200
    );
    assert_eq!(claim_provider(&conn, ticket), None);
}

#[test]
fn a_standby_takes_work_during_a_hold_and_yields_at_reset() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [100, 0, 0], false);
    file(
        &mut conn,
        &[
            ("one", &[], &[]),
            ("two", &[], &[]),
            ("three", &[], &[]),
            ("four", &[], &[]),
        ],
    );
    let machine = machine_room("m-1", 4, &["claude", "codex", "third"]);
    assert_eq!(ask(&mut conn, &machine, start())["provider"], "claude");
    assert_eq!(ask(&mut conn, &machine, at(1.0))["provider"], "claude");
    hold_claude(&conn, "5-hour", at(5.0));
    assert_eq!(ask(&mut conn, &machine, at(2.0))["provider"], "codex");
    assert_eq!(ask(&mut conn, &machine, at(5.0))["provider"], "claude");
}

#[test]
fn a_pin_overrides_share_but_still_waits_for_holds_and_enabled_providers() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [0, 100, 100], false);
    let tickets = file(
        &mut conn,
        &[
            ("pinned standby", &["provider:Claude"], &[]),
            ("pinned off", &["provider:third"], &[0]),
        ],
    );
    let machine = machine_room("m-1", 2, &["claude", "codex", "third"]);
    hold_claude(&conn, "5-hour", at(10.0));
    let reply = ask(&mut conn, &machine, at(1.0));
    let reason = reply["nothing"].as_str().unwrap();
    assert!(
        reason.contains(&format!("claude is held until {}", at(10.0))),
        "{reason}"
    );
    assert!(
        reason.contains("codex is excluded by the provider pin"),
        "{reason}"
    );
    let reserved = ask(&mut conn, &machine, at(10.0));
    assert_eq!(reserved["provider"], "claude");
    let session = "cli-claude-ea631acc";
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&reserved, session),
            at(11.0)
        )
        .0,
        200
    );
    assert_eq!(
        post(
            &mut conn,
            "/landed",
            &landed(tickets[0].as_str(), session, "done"),
            at(12.0)
        )
        .0,
        200
    );
    assert_eq!(claim_provider(&conn, tickets[0].as_str()), None);
    let reply = ask(&mut conn, &machine, at(13.0));
    assert!(reply["nothing"].as_str().unwrap().contains("third is off"));
}

#[test]
fn the_machine_only_gets_providers_it_offers_and_the_reason_names_absent_ones() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [100, 50, 0], true);
    file(&mut conn, &[("one", &[], &[]), ("two", &[], &[])]);
    let machine = machine_room("m-1", 2, &["third"]);
    assert_eq!(ask(&mut conn, &machine, start())["provider"], "third");
    hold_claude(&conn, "5-hour", at(10.0));
    let machine = machine_room("m-1", 2, &["claude"]);
    let reply = ask(&mut conn, &machine, at(1.0));
    let reason = reply["nothing"].as_str().unwrap();
    assert!(reason.contains("claude is held until"), "{reason}");
    assert!(
        reason.contains("codex has no CLI on this machine"),
        "{reason}"
    );
    assert!(
        reason.contains("third has no CLI on this machine"),
        "{reason}"
    );
}

#[test]
fn new_settings_apply_to_the_next_reservation_without_changing_existing_ones() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [100, 0, 0], true);
    file(
        &mut conn,
        &[("one", &[], &[]), ("two", &[], &[]), ("three", &[], &[])],
    );
    let machine = machine_room("m-1", 2, &["claude", "codex", "third"]);
    let first = ask(&mut conn, &machine, start());
    assert_eq!(first["provider"], "claude");
    set_mix(&mut conn, [0, 100, 0], true);
    assert_eq!(ask(&mut conn, &machine, at(1.0))["provider"], "codex");
    let ticket = first["ticket"].as_str().unwrap();
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&first, "cli-claude-ea631acc"),
            at(2.0)
        )
        .0,
        200
    );
    assert_eq!(claim_provider(&conn, ticket).as_deref(), Some("claude"));
}

#[test]
fn ordinary_work_waits_for_pending_reservations_and_regressions_bypass_room() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [100, 0, 0], true);
    let tickets = file(
        &mut conn,
        &[("one", &[], &[]), ("two", &[], &[]), ("fix", &[], &[])],
    );
    let mut machine = machine_room("m-1", 1, &["claude", "codex"]);
    let first = ask(&mut conn, &machine, start());
    let reply = ask(&mut conn, &machine, at(1.0));
    assert!(
        reply["nothing"]
            .as_str()
            .unwrap()
            .contains("room to start another worker")
    );
    let fix = tickets
        .iter()
        .find(|ticket| Some(ticket.as_str()) != first["ticket"].as_str())
        .unwrap();
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('rust:a::b', ?1, 'b1', 'a1', 'open')",
        [fix.as_str()],
    )
    .unwrap();
    machine["room"]["memory_pressed"] = json!(true);
    let reply = ask(&mut conn, &machine, at(2.0));
    assert!(
        reply["nothing"]
            .as_str()
            .unwrap()
            .contains("memory pressure")
    );
    machine["room"]["memory_pressed"] = json!(false);
    assert_eq!(ask(&mut conn, &machine, at(3.0))["ticket"], fix.as_str());
}

#[test]
fn a_lane_continues_on_its_provider_even_after_the_shares_change() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [100, 0, 0], true);
    let tickets = file(
        &mut conn,
        &[("one", &["lane:port"], &[]), ("two", &["lane:port"], &[0])],
    );
    let machine = machine_room("m-1", 1, &["claude", "codex"]);
    let first = ask(&mut conn, &machine, start());
    assert_eq!(first["provider"], "claude");
    let session = "cli-claude-ea631acc";
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&first, session),
            at(1.0)
        )
        .0,
        200
    );
    set_mix(&mut conn, [0, 100, 0], true);
    let landing = landed(tickets[0].as_str(), session, "done");
    let (status, reply) = post(&mut conn, "/landed", &landing, at(2.0));
    assert_eq!(status, 200);
    assert_eq!(reply["next"]["ticket"], tickets[1].as_str());
    assert_eq!(reply["next"]["session"], session);
    assert_eq!(reply["next"]["provider"], "claude");
    assert_eq!(post(&mut conn, "/landed", &landing, at(3.0)).1, reply);
}

#[test]
fn a_family_hold_uses_the_model_under_the_tier() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [100, 0, 0], true);
    file(
        &mut conn,
        &[
            ("one", &["tier:opus"], &[]),
            ("two", &["tier:sonnet"], &[0]),
        ],
    );
    hold_claude(&conn, "Weekly · Sonnet", at(10.0));
    let mut machine = machine_room("m-1", 2, &["claude", "codex"]);
    machine["room"]["providers"][0]["model_ids"] =
        json!({"opus": "claude-sonnet-5", "sonnet": "claude-opus-5"});
    let first = ask(&mut conn, &machine, at(1.0));
    assert_eq!(first["provider"], "codex");
    let session = "cli-codex-ea631acc";
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&first, session),
            at(2.0)
        )
        .0,
        200
    );
    assert_eq!(
        post(
            &mut conn,
            "/landed",
            &landed(first["ticket"].as_str().unwrap(), session, "done"),
            at(3.0)
        )
        .0,
        200
    );
    assert_eq!(ask(&mut conn, &machine, at(4.0))["provider"], "claude");
}

#[test]
fn an_older_daemon_gets_its_idle_worker_and_can_retry() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [0, 100, 0], true);
    file(&mut conn, &[("one", &[], &[]), ("two", &[], &[])]);
    let machine = free("m-1", &["local_a"]);
    let first = ask(&mut conn, &machine, start());
    assert_eq!(first["session"], "local_a");
    assert!(first.get("provider").is_none());
    assert_eq!(ask(&mut conn, &machine, at(1.0)), first);
}

#[test]
fn an_existing_claim_counts_when_its_worker_reported_the_provider() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [50, 50, 0], true);
    let tickets = file(&mut conn, &[("one", &[], &[]), ("two", &[], &[])]);
    let ticket = tickets[0].as_str();
    let session = "cli-claude-ea631acc";
    let claim = super::tests::claim(ticket, session);
    assert_eq!(post(&mut conn, "/claims", &claim, start()).0, 200);
    assert_eq!(claim_provider(&conn, ticket), None);
    conn.execute(
        "INSERT INTO workers (machine, session, app, role, state, ticket)
         VALUES ('m-1', ?1, 'claude', 'worker', 'working', ?2)",
        [session, ticket],
    )
    .unwrap();
    let machine = machine_room("m-2", 1, &["claude", "codex"]);
    assert_eq!(ask(&mut conn, &machine, at(1.0))["provider"], "codex");
}

#[test]
fn refusing_a_reservation_frees_its_ticket_and_a_stale_refusal_is_harmless() {
    use super::tests::{free, post};
    use ignite_store::hub;

    let mut conn = hub::open_in_memory().unwrap();
    let tickets = super::tests::file(&mut conn, &[("ticket", &[], &[])]);
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["first"]), start());
    assert_eq!(first["ticket"], tickets[0].as_str());
    let mut wrong_code = first.clone();
    wrong_code["code"] = json!("different-code");
    assert_eq!(
        post(&mut conn, "/reservations/refuse", &wrong_code, start()).0,
        200
    );
    let (_, still_reserved) = post(&mut conn, "/next", &free("m-2", &["second"]), start());
    assert!(still_reserved.is_null());
    assert_eq!(
        post(&mut conn, "/reservations/refuse", &first, start()).0,
        200
    );
    let (_, second) = post(&mut conn, "/next", &free("m-2", &["second"]), start());
    assert_eq!(second["ticket"], first["ticket"]);
    assert_eq!(
        post(&mut conn, "/reservations/refuse", &first, start()).0,
        200
    );
    let mut claim = super::tests::claim_reserved(&second, "second");
    claim["machine"] = second["machine"].clone();
    assert_eq!(post(&mut conn, "/claims", &claim, start()).0, 200);
}
