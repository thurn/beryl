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

fn hold_provider(conn: &Connection, provider: &str, label: &str, reset: Timestamp) {
    provider_usage::record(
        conn,
        &ProviderSample {
            provider: provider.to_owned(),
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
fn releasing_a_claim_and_lapsing_a_reservation_each_free_provider_load() {
    use ignite_store::hub::fleet::RESERVATION_SECONDS;

    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [50, 50, 0], false);
    file(
        &mut conn,
        &[("one", &[], &[]), ("two", &[], &[]), ("three", &[], &[])],
    );
    let machine = machine_room("m-1", 3, &["claude", "codex"]);
    let session = "cli-claude-ea631acc";
    let first = ask(&mut conn, &machine, start());
    assert_eq!(first["provider"], "claude");
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&first, session),
            start()
        )
        .0,
        200
    );
    assert_eq!(ask(&mut conn, &machine, at(1.0))["provider"], "codex");
    assert_eq!(ask(&mut conn, &machine, at(2.0))["provider"], "claude");

    let ticket = first["ticket"].as_str().unwrap();
    assert_eq!(
        post(&mut conn, "/release", &release(ticket, session), at(3.0)).0,
        200
    );
    let after_release = ask(&mut conn, &machine, at(4.0));
    assert_eq!(after_release["provider"], "claude");
    assert_eq!(
        post(&mut conn, "/reservations/refuse", &after_release, at(4.0)).0,
        200
    );

    // Only the Codex reservation has lapsed. The remaining Claude reservation still counts.
    let after_lapse = ask(&mut conn, &machine, at(RESERVATION_SECONDS + 1.0));
    assert_eq!(after_lapse["provider"], "codex");
}

#[test]
fn an_older_daemon_cannot_start_work_with_a_disabled_provider() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [50, 50, 0], false);
    let ticket = file(&mut conn, &[("work", &[], &[])]).remove(0);
    let mut machine = free("m-1", &["third-idle"]);
    machine["idle_workers"][0]["app"] = json!("third");
    machine["explain"] = json!(true);
    assert_eq!(ask(&mut conn, &machine, start())["nothing"], "third is off");
    machine["idle_workers"][0]["app"] = json!("claude");
    assert_eq!(ask(&mut conn, &machine, at(1.0))["ticket"], ticket.as_str());
}

#[test]
fn the_third_provider_takes_work_during_holds_and_yields_at_reset() {
    let mut conn = open_in_memory().unwrap();
    set_mix(&mut conn, [60, 40, 0], true);
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
    hold_provider(&conn, "claude", "5-hour", at(5.0));
    assert_eq!(ask(&mut conn, &machine, at(2.0))["provider"], "codex");
    hold_provider(&conn, "codex", "5-hour", at(10.0));
    assert_eq!(ask(&mut conn, &machine, at(3.0))["provider"], "third");
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
    hold_provider(&conn, "claude", "5-hour", at(10.0));
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
    hold_provider(&conn, "claude", "5-hour", at(10.0));
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
    assert_ne!(first["master_blocker"], json!(true));
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
    let repair = ask(&mut conn, &machine, at(3.0));
    assert_eq!(repair["ticket"], fix.as_str());
    assert_eq!(repair["master_blocker"], json!(true));
    let stored = ignite_store::hub::fleet::reservation_for_worker(
        &conn,
        "m-1",
        repair["session"].as_str().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(stored.master_blocker);
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
    hold_provider(&conn, "claude", "Weekly · Sonnet", at(10.0));
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
fn a_batch_claim_cannot_take_a_live_reservation() {
    use ignite_core::protocol::tickets::{BatchRequest, Operation};
    use ignite_store::hub::batch;

    let mut conn = open_in_memory().unwrap();
    let id = file(&mut conn, &[("work", &[], &[])]).remove(0);
    let machine = machine_room("m-1", 1, &["claude"]);
    let reserved = ask(&mut conn, &machine, start());
    assert_eq!(reserved["ticket"], id.as_str());

    let batch = BatchRequest {
        preconditions: vec![],
        ops: vec![Operation::Claim { id: id.to_string() }],
    };
    assert!(matches!(
        batch::apply(&mut conn, &batch, "batch-actor", "m-2", at(1.0)).unwrap(),
        batch::BatchOutcome::Conflict(_)
    ));
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&reserved, "cli-claude-12345678"),
            at(2.0)
        )
        .0,
        200
    );
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

fn repair_world() -> (Connection, ignite_core::ids::TicketId) {
    let mut conn = open_in_memory().unwrap();
    super::tests::register(&mut conn);
    let machine = json!({"machine": "m-2", "name": "remote", "os": "macos",
                         "cores": 8, "memory_mb": 16384, "families": [], "version": "v"});
    assert_eq!(
        post(&mut conn, "/machines/register", &machine, start()).0,
        200
    );
    let culprit = file(&mut conn, &[("culprit landing", &[], &[])]).remove(0);
    let session = "cli-codex-12345678";
    let claim = super::tests::claim(culprit.as_str(), session);
    assert_eq!(post(&mut conn, "/claims", &claim, start()).0, 200);
    assert_eq!(
        post(
            &mut conn,
            "/landed",
            &landed(culprit.as_str(), session, "done"),
            start()
        )
        .0,
        200
    );
    // With no transcript to consider, the mix would give the ticket to Claude.
    set_mix(&mut conn, [100, 0, 0], false);
    (conn, culprit)
}

fn file_repair(
    conn: &mut Connection,
    culprit: Option<&ignite_core::ids::TicketId>,
    now: Timestamp,
) -> String {
    use crate::upkeep::{filing::Author, regressions};
    use ignite_core::protocol::hub::{FoundBy, RegressionReport};

    let report = RegressionReport {
        check: "rust:a".parse().unwrap(),
        first_bad: if culprit.is_some() {
            "3a9e4c1"
        } else {
            "abcdef1"
        }
        .parse()
        .unwrap(),
        last_good: "abcdef0".parse().unwrap(),
        caused_by: culprit.cloned(),
        output: "check failed".to_owned(),
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by: FoundBy::Landing,
        detected_at: Some("abcdef1".parse().unwrap()),
    };
    let author = Author {
        actor: "m-2",
        machine: "m-2",
    };
    regressions::file(conn, author, &report, now).unwrap().0
}

#[test]
fn a_late_culprit_reroutes_an_unclaimed_regression_to_its_machine_and_provider() {
    use ignite_core::protocol::tickets::CAUSED_BY_KEY;
    use ignite_store::hub::tickets;

    let (mut conn, culprit) = repair_world();
    let id = file_repair(&mut conn, None, at(1.0));
    let original = ask(&mut conn, &machine_room("m-2", 1, &["claude"]), at(2.0));
    assert_eq!(original["ticket"], id);
    assert_eq!(file_repair(&mut conn, Some(&culprit), at(3.0)), id);
    let mut stale_claim = claim_reserved(&original, "cli-claude-12345678");
    stale_claim["machine"] = json!("m-2");
    assert_eq!(post(&mut conn, "/claims", &stale_claim, at(4.0)).0, 409);
    let ticket = tickets::load(&conn, &id).unwrap().unwrap();
    assert_eq!(ticket.metadata[CAUSED_BY_KEY], json!(culprit));
    assert!(ticket.labels.iter().any(|label| label == "machine:jarvis"));

    let nearby = machine_room("m-1", 1, &["claude", "codex"]);
    let elsewhere = machine_room("m-2", 1, &["claude"]);
    assert_eq!(
        ask(&mut conn, &elsewhere, at(1799.0))["ticket"],
        Value::Null
    );
    let resumed = ask(&mut conn, &nearby, at(1799.0));
    assert_eq!(resumed["ticket"], id);
    assert_eq!(resumed["provider"], "codex");
    assert_eq!(
        post(&mut conn, "/reservations/refuse", &resumed, at(1799.0)).0,
        200
    );

    // The last ask is not a heartbeat. At 30 minutes the original provider is no longer required.
    let fresh = ask(&mut conn, &elsewhere, at(1800.0));
    assert_eq!(fresh["ticket"], id);
    assert_eq!(fresh["provider"], "claude");
}

#[test]
fn a_late_culprit_tells_the_worker_that_already_claimed_the_ticket() {
    use ignite_core::protocol::tickets::{CAUSED_BY_KEY, Status};
    use ignite_store::hub::tickets;

    let (mut conn, culprit) = repair_world();
    let id = file_repair(&mut conn, None, at(1.0));
    let reservation = ask(&mut conn, &machine_room("m-2", 1, &["claude"]), at(2.0));
    let worker = "cli-claude-12345678";
    let mut claim = claim_reserved(&reservation, worker);
    claim["machine"] = json!("m-2");
    assert_eq!(post(&mut conn, "/claims", &claim, at(3.0)).0, 200);
    assert_eq!(file_repair(&mut conn, Some(&culprit), at(4.0)), id);
    // Retrying the same evidence produces no extra message.
    assert_eq!(file_repair(&mut conn, Some(&culprit), at(5.0)), id);

    let ticket = tickets::load(&conn, &id).unwrap().unwrap();
    assert_eq!(ticket.status, Status::InProgress);
    assert_eq!(ticket.assignee.as_deref(), Some(worker));
    assert_eq!(ticket.metadata[CAUSED_BY_KEY], json!(culprit));
    let comments = tickets::comments(&conn, &id).unwrap();
    let tells: Vec<_> = comments
        .iter()
        .filter(|c| c.text.contains("The culprit search"))
        .collect();
    assert_eq!(tells.len(), 1);
    let tell = tells[0];
    assert_eq!(ticket.metadata["tell"], json!(tell.n));
    assert!(tell.text.contains(culprit.as_str()));
}

#[test]
fn a_reverted_cli_landing_keeps_its_routing_until_it_lands_again() {
    use crate::upkeep::{filing::Author, reverted};
    use ignite_core::protocol::hub::Reverted;
    use ignite_core::protocol::tickets::{CULPRIT_AFFINITY_KEY, REVERTED_KEY};
    use ignite_core::tipround::CulpritSearch;
    use ignite_store::hub::tickets;

    let (mut conn, culprit) = repair_world();
    let report = Reverted {
        ticket: Some(culprit.clone()),
        landed: vec!["3a9e4c1".parse().unwrap()],
        revert: "c0dec01".parse().unwrap(),
        checks: vec!["rust:a".parse().unwrap()],
        output: "failed to build".to_owned(),
        search: CulpritSearch::OneCandidate,
        machine: "remote".to_owned(),
    };
    let author = Author {
        actor: "m-2",
        machine: "m-2",
    };
    let id = reverted::file(&mut conn, author, &report, at(1.0)).unwrap();
    let elsewhere = ask(&mut conn, &machine_room("m-2", 1, &["claude"]), at(2.0));
    assert_eq!(elsewhere["ticket"], Value::Null);
    let nearby = ask(
        &mut conn,
        &machine_room("m-1", 1, &["claude", "codex"]),
        at(3.0),
    );
    assert_eq!(nearby["ticket"], id);
    assert_eq!(nearby["provider"], "codex");
    let worker = "cli-codex-12345679";
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&nearby, worker),
            at(4.0)
        )
        .0,
        200
    );
    assert_eq!(
        post(
            &mut conn,
            "/landed",
            &landed(&id, worker, "repaired"),
            at(5.0)
        )
        .0,
        200
    );

    let ticket = tickets::load(&conn, &id).unwrap().unwrap();
    assert!(!ticket.labels.iter().any(|label| label == "machine:jarvis"));
    assert!(!ticket.metadata.contains_key(CULPRIT_AFFINITY_KEY));
    assert!(!ticket.metadata.contains_key(REVERTED_KEY));
}
