//! Placement through `POST /next` with registered machines and synthetic heartbeats.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::capacity::{CapacityReport, ProviderCapacity};
use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_core::fleet::{MachineState, ProviderSample};
use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::protocol::machine_state::MachineSnapshot;
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, machine_snapshots, placement_deferrals};

use super::tests::{at, claim, file, free, landed, post, register, start};

const OUTPUT: &str = "engine/target";

fn machines() -> Connection {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let second = json!({"machine": "m-2", "name": "remote", "os": "macos",
                        "cores": 20, "memory_mb": 16384, "families": [], "version": "v"});
    assert_eq!(
        post(&mut conn, "/machines/register", &second, start()).0,
        200
    );
    conn
}

pub(super) fn capacity(warm: bool) -> CapacityReport {
    CapacityReport {
        outputs: vec![OUTPUT.into()],
        leased_trees: warm
            .then(|| ("donor".parse().unwrap(), vec![OUTPUT.into()]))
            .into_iter()
            .collect(),
        worker_room: 2,
        admit_cores: 20.0,
        free_cores: 10.0,
        providers: vec![ProviderCapacity {
            name: "claude".into(),
            tiers: vec!["sonnet".into()],
            probed: true,
            hold_reason: None,
            model_ids: std::collections::BTreeMap::new(),
            default_model: None,
        }],
        ..CapacityReport::default()
    }
}

pub(super) fn record(conn: &Connection, machine: &str, report: &CapacityReport, now: Timestamp) {
    let snapshot = MachineSnapshot {
        at: now,
        free_disk_gib: None,
        reclaimable: None,
        next_ticket: None,
        next_ticket_disk_gib: None,
        start_block: None,
        start_reason: None,
        cpu_percent: None,
        queued_cores: 0,
        swap_used_mb: None,
        idle_workers: 0,
        working_workers: 0,
        stalled_workers: 0,
        down_workers: 0,
        ready_tickets: 0,
    };
    machine_snapshots::record(conn, machine, &snapshot, Some(report)).unwrap();
}

fn request(machine: &str, report: &CapacityReport, queue_seconds: u32) -> Value {
    let mut ask = free(machine, &[]);
    ask["explain"] = json!(true);
    ask["free_cores"] = json!(report.free_cores);
    ask["admit_cores"] = json!(report.admit_cores);
    ask["room"] = json!({"start": 2, "providers": [{"name": "claude", "tiers": ["sonnet"]}]});
    ask["pressure"] = json!(DispatchPressure {
        outputs: report.outputs.clone(),
        free_trees: report.free_trees.clone(),
        leased: report.leased_trees.clone(),
        queue_seconds,
        ..DispatchPressure::default()
    });
    ask
}

fn ask(conn: &mut Connection, who: &Value, now: Timestamp) -> Value {
    let (status, reply) = post(conn, "/next", who, now);
    assert_eq!(status, 200, "{reply}");
    reply
}

fn files(conn: &Connection, id: &TicketId, path: &str) {
    conn.execute(
        "UPDATE tickets SET body = ?2 WHERE id = ?1",
        (id.as_str(), format!("Edit `{path}`.")),
    )
    .unwrap();
}

fn build_ticket(conn: &mut Connection, labels: &[&str]) -> TicketId {
    let id = file(conn, &[("change the engine", labels, &[])]).remove(0);
    files(conn, &id, "engine/src/main.rs");
    id
}

#[test]
fn the_warm_machine_takes_the_ticket_and_reservation_clears_the_deferral() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    let warm = capacity(true);
    record(&conn, "m-2", &warm, start());
    // The current ask overrides the asker's earlier report of a warm output.
    record(&conn, "m-1", &warm, start());
    let cold = request("m-1", &capacity(false), 0);
    let waiting = ask(&mut conn, &cold, at(1.0));
    assert_eq!(
        waiting["nothing"],
        format!(
            "{ticket} is better placed on remote: it holds {OUTPUT}, and here they would build cold (about 100 s)"
        )
    );
    assert_eq!(
        placement_deferrals::first(&conn, &ticket).unwrap(),
        Some(at(1.0))
    );
    record(&conn, "m-1", &capacity(false), at(1.0));
    let reserved = ask(&mut conn, &request("m-2", &warm, 0), at(2.0));
    assert_eq!(reserved["ticket"], ticket.as_str(), "{reserved}");
    assert_eq!(placement_deferrals::first(&conn, &ticket).unwrap(), None);
}

#[test]
fn the_reason_uses_the_selected_report_when_machine_names_repeat() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    conn.execute(
        "UPDATE tickets SET needs_os = 'macos' WHERE id = ?1",
        [ticket.as_str()],
    )
    .unwrap();
    let other = json!({"machine": "m-0", "name": "remote", "os": "windows",
                        "cores": 20, "memory_mb": 16384, "families": [], "version": "v"});
    assert_eq!(
        post(&mut conn, "/machines/register", &other, start()).0,
        200
    );
    record(&conn, "m-0", &capacity(false), start());
    record(&conn, "m-2", &capacity(true), start());
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(
        ask(&mut conn, &cold, start())["nothing"],
        format!(
            "{ticket} is better placed on remote: it holds {OUTPUT}, and here they would build cold (about 100 s)"
        )
    );
}

#[test]
fn after_sixty_seconds_the_cold_machine_takes_a_deferred_ticket() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    let warm = capacity(true);
    record(&conn, "m-2", &warm, start());
    record(&conn, "m-2", &warm, at(50.0));
    let cold = request("m-1", &capacity(false), 0);
    assert!(ask(&mut conn, &cold, start())["nothing"].is_string());
    assert!(ask(&mut conn, &cold, at(59.999))["nothing"].is_string());
    assert_eq!(
        placement_deferrals::first(&conn, &ticket).unwrap(),
        Some(start())
    );
    assert_eq!(ask(&mut conn, &cold, at(60.0))["ticket"], ticket.as_str());
    assert_eq!(placement_deferrals::first(&conn, &ticket).unwrap(), None);
}

#[test]
fn closing_and_reopening_a_ticket_restarts_its_placement_wait() {
    use ignite_core::protocol::tickets::{BatchRequest, Operation};
    use ignite_store::hub::batch;

    let mut conn = machines();
    let ticket = super::tests::file_under(
        &mut conn,
        super::tests::Parent::Loose,
        &[("change the engine", &[], &[])],
    )
    .1
    .remove(0);
    files(&conn, &ticket, "engine/src/main.rs");
    let cold = request("m-1", &capacity(false), 0);
    let warm = capacity(true);
    record(&conn, "m-2", &warm, start());
    assert!(ask(&mut conn, &cold, start())["nothing"].is_string());
    assert_eq!(
        placement_deferrals::first(&conn, &ticket).unwrap(),
        Some(start())
    );
    let ops = vec![
        Operation::Close {
            id: ticket.to_string(),
            reason: "deferred work".into(),
            include_ready: false,
        },
        Operation::Reopen {
            id: ticket.to_string(),
        },
    ];
    assert!(matches!(
        batch::apply(
            &mut conn,
            &BatchRequest {
                preconditions: vec![],
                ops
            },
            "operator",
            "m-1",
            at(61.0)
        )
        .unwrap(),
        batch::BatchOutcome::Applied(_)
    ));
    assert_eq!(placement_deferrals::first(&conn, &ticket).unwrap(), None);
    record(&conn, "m-2", &warm, at(61.0));
    assert!(ask(&mut conn, &cold, at(61.0))["nothing"].is_string());
    assert_eq!(
        placement_deferrals::first(&conn, &ticket).unwrap(),
        Some(at(61.0))
    );
}

#[test]
fn a_stale_report_cannot_defer_a_ticket() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    record(&conn, "m-2", &capacity(true), start());
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(ask(&mut conn, &cold, at(60.001))["ticket"], ticket.as_str());
}

#[test]
fn dispatch_continues_to_the_next_ticket_after_deferring_a_build() {
    let mut conn = machines();
    let ids = file(
        &mut conn,
        &[
            ("change the engine", &[], &[]),
            ("update the docs", &[], &[]),
        ],
    );
    let [build, docs] = ids.as_slice() else {
        panic!("two tickets were filed")
    };
    files(&conn, build, "engine/src/main.rs");
    conn.execute(
        "UPDATE tickets SET priority = 0 WHERE id = ?1",
        [build.as_str()],
    )
    .unwrap();
    files(&conn, docs, "docs/overview.md");
    record(&conn, "m-2", &capacity(true), start());
    let cold = request("m-1", &capacity(false), 1);
    assert_eq!(ask(&mut conn, &cold, start())["ticket"], docs.as_str());
    assert_eq!(
        placement_deferrals::first(&conn, build).unwrap(),
        Some(start())
    );
}

#[test]
fn a_ticket_without_outputs_can_start_on_either_machine_within_the_margin() {
    for asker in ["m-1", "m-2"] {
        let mut conn = machines();
        let docs = file(&mut conn, &[("update the docs", &[], &[])]).remove(0);
        files(&conn, &docs, "docs/overview.md");
        for machine in ["m-1", "m-2"] {
            record(&conn, machine, &capacity(true), start());
        }
        let request = request(asker, &capacity(false), 1);
        assert_eq!(ask(&mut conn, &request, start())["ticket"], docs.as_str());
    }
}

#[test]
fn equal_build_times_favor_the_larger_fraction_of_free_cores() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    let a = capacity(true);
    let b = CapacityReport {
        free_cores: 8.0,
        admit_cores: 10.0,
        ..capacity(true)
    };
    record(&conn, "m-1", &a, start());
    record(&conn, "m-2", &b, start());
    assert_eq!(
        ask(&mut conn, &request("m-1", &a, 0), start())["nothing"],
        format!("{ticket} is better placed on remote: it has a larger share of its cores free")
    );
    assert_eq!(
        ask(&mut conn, &request("m-2", &b, 0), start())["ticket"],
        ticket.as_str()
    );
}

#[test]
fn a_peer_without_a_provider_for_the_ticket_cannot_defer_it() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &["provider:claude"]);
    let mut warm = capacity(true);
    warm.providers[0].hold_reason = Some("plan_limit".into());
    record(&conn, "m-2", &warm, start());
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(ask(&mut conn, &cold, start())["ticket"], ticket.as_str());
}

#[test]
fn a_peer_uses_its_own_model_ids_to_check_usage_holds() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &["tier:sonnet"]);
    let mut warm = capacity(true);
    warm.providers[0]
        .model_ids
        .insert("sonnet".into(), "claude-opus".into());
    record(&conn, "m-2", &warm, start());
    hub::provider_usage::record(
        &conn,
        &ProviderSample {
            provider: "claude".into(),
            at: start(),
            machine: "m-2".parse().unwrap(),
            status: "ok".into(),
            label: Some("Weekly · Opus".into()),
            percent_used: Some(99.0),
            resets_at: Some(at(30.0).to_string()),
            note: None,
        },
    )
    .unwrap();
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(ask(&mut conn, &cold, at(1.0))["ticket"], ticket.as_str());
}

#[test]
fn a_checks_only_peer_with_a_fresh_report_cannot_defer_work() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    record(&conn, "m-2", &capacity(true), start());
    hub::fleet_settings::set_machine_state(
        &mut conn,
        "m-2",
        MachineState::ChecksOnly,
        "tester",
        start(),
    )
    .unwrap();
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(ask(&mut conn, &cold, at(1.0))["ticket"], ticket.as_str());
}

#[test]
fn a_new_reservation_can_fill_the_peers_reported_worker_room() {
    let mut conn = machines();
    let warm = CapacityReport {
        worker_room: 1,
        ..capacity(true)
    };
    record(&conn, "m-2", &warm, start());
    let first = build_ticket(&mut conn, &[]);
    assert_eq!(
        ask(&mut conn, &request("m-2", &warm, 0), start())["ticket"],
        first.as_str()
    );
    let second = build_ticket(&mut conn, &[]);
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(ask(&mut conn, &cold, at(1.0))["ticket"], second.as_str());
}

#[test]
fn pinned_and_justiciar_tickets_skip_placement() {
    for label in ["machine:jarvis", "justiciar"] {
        let mut conn = machines();
        let ticket = build_ticket(&mut conn, &[label]);
        record(&conn, "m-2", &capacity(true), start());
        let cold = request("m-1", &capacity(false), 0);
        assert_eq!(ask(&mut conn, &cold, start())["ticket"], ticket.as_str());
        assert_eq!(placement_deferrals::first(&conn, &ticket).unwrap(), None);
    }
}

#[test]
fn an_unpinned_regression_skips_placement() {
    let mut conn = machines();
    let ticket = build_ticket(&mut conn, &[]);
    hub::tips::regression(
        &conn,
        &RegressionReport {
            check: "build:engine".parse().unwrap(),
            first_bad: "abcdef1".parse().unwrap(),
            last_good: "abcdef0".parse().unwrap(),
            caused_by: None,
            output: "failed to build".into(),
            suspects: Vec::new(),
            platform: None,
            search: None,
            found_by: FoundBy::Landing,
            detected_at: None,
        },
        ticket.as_str(),
    )
    .unwrap();
    record(&conn, "m-2", &capacity(true), start());
    let cold = request("m-1", &capacity(false), 0);
    assert_eq!(ask(&mut conn, &cold, start())["ticket"], ticket.as_str());
    assert_eq!(placement_deferrals::first(&conn, &ticket).unwrap(), None);
}

#[test]
fn the_lane_successor_is_reserved_for_the_session_that_landed() {
    let mut conn = machines();
    let lane = ["lane:engine"];
    let ids = file(&mut conn, &[("first", &lane, &[]), ("second", &lane, &[0])]);
    files(&conn, &ids[1], "engine/src/main.rs");
    record(&conn, "m-2", &capacity(true), start());
    let session = "cli-claude-ea631acc";
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
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), session, "done"),
        at(1.0),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["next"]["ticket"], ids[1].as_str());
    assert_eq!(reply["next"]["session"], session);
    assert_eq!(placement_deferrals::first(&conn, &ids[1]).unwrap(), None);
}
