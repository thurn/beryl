//! The flaky-check guards, machines that disagree, and tickets no machine can take, against an
//! in-memory hub database with time from a fake clock.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    CheckState, FlakyReport, MachineRegister, NewResult, ResultKey, StoreRequest,
};
use ignite_core::protocol::tickets::{Operation, Status, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub::{self, checks, fleet, results};

use super::tests::{hold_sources, all_tickets, apply, check_state, comment_count, list_check, start, ticket};
use super::{Upkeep, machines};
use crate::auth::Writer;
use crate::routes::{Request, checks as check_routes};

fn flaky(check: &str, commit: &str, command: &str) -> FlakyReport {
    FlakyReport {
        check: CheckName::new(check).unwrap(),
        hash: InputHash::new("1".repeat(64)).unwrap(),
        commit: Commit::new(commit).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        always: false,
        command: command.to_owned(),
        output: format!("{check} failed in {command}"),
    }
}

fn imported_flake(conn: &Connection, check: &str, hash: &str, filed: Option<&str>, now: Timestamp) {
    conn.execute(
        "INSERT INTO flaky (check_name, hash, commit_sha, machine, at, filed)
         VALUES (?1, ?2, 'aaaa', 'legacy-host', ?3, ?4)",
        params![check, hash, now.to_string(), filed],
    )
    .unwrap();
}

fn imported_remedy(conn: &Connection, id: &str, category: &str, checks: &[&str], now: Timestamp) {
    let metadata = serde_json::json!({
        "ignite_fault": {"category": category},
        "ignite_checks": checks,
    });
    conn.execute(
        "INSERT INTO tickets (id, title, status, created_at, updated_at, created_by, metadata, version)
         VALUES (?1, 'Imported flakes', 'closed', ?2, ?2, 'ignite-upkeep', ?3, 0)",
        params![id, now.to_string(), metadata.to_string()],
    )
    .unwrap();
}

fn register(conn: &Connection, id: &str, name: &str, os: Os, now: Timestamp) {
    let machine = MachineRegister {
        machine: MachineId::new(id).unwrap(),
        name: name.to_owned(),
        os,
        cores: 16,
        memory_mb: 65_536,
        families: Vec::new(),
        version: "v".to_owned(),
    };
    fleet::register(conn, &machine, now).unwrap();
}

#[test]
fn an_always_set_check_that_flakes_once_stays_on_and_gets_one_p1() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    list_check(&mut conn, "format:rust");
    let always = |commit: &str| FlakyReport {
        always: true,
        ..flaky("format:rust", commit, &format!("m-1:{commit}"))
    };

    checks::record_flaky(&conn, &always("aaaa"), clock.now()).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_switched_off.is_empty());
    assert_eq!(check_state(&conn, "format:rust"), "on");
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert_eq!(filed.priority, 1);
    assert!(filed.body.contains("format:rust failed in m-1:aaaa"));

    // The records it was filed for no longer count, so later passes file nothing more.
    clock.advance(60.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.tickets_filed.is_empty());
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
}

#[test]
fn many_checks_flaking_in_one_command_switch_none_off_and_file_one_machine_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    register(&conn, "m-1", "jarvis", Os::Macos, clock.now());
    let names = ["ui:a", "ui:b", "rust:c"];
    for name in names {
        list_check(&mut conn, name);
    }

    for name in names {
        checks::record_flaky(&conn, &flaky(name, "aaaa", "m-1:run-1"), clock.now()).unwrap();
    }
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let machine = ticket(&conn, &report.tickets_filed[0]);
    assert_eq!(machine.priority, 1);
    assert!(machine.labels.contains(&"machine:jarvis".to_owned()));
    assert!(machine.body.contains("ui:a, ui:b"));

    // The same machine does it again on another commit: still no check is switched off, and
    // the machine's ticket gets a comment instead of a second ticket.
    clock.advance(86_400.0);
    for name in names {
        checks::record_flaky(&conn, &flaky(name, "bbbb", "m-1:run-2"), clock.now()).unwrap();
    }
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_switched_off.is_empty());
    assert_eq!(report.tickets_filed, vec![machine.id.to_string()]);
    assert_eq!(comment_count(&conn, machine.id.as_str()), 1);

    // One of them flaking alone later is its first record that counts.
    clock.advance(86_400.0);
    checks::record_flaky(&conn, &flaky("ui:a", "cccc", "m-1:run-3"), clock.now()).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.checks_switched_off, ["ui:a"]);
    assert_eq!(check_state(&conn, "ui:a"), "off");
    assert_eq!(check_state(&conn, "ui:b"), "on");
    assert_eq!(check_state(&conn, "rust:c"), "on");
    assert_eq!(all_tickets(&conn, clock.now()).len(), 2);
}

#[test]
fn imported_flakes_stay_visible_and_only_the_fresh_command_switches_a_check_off() {
    let clock = start();
    let now = clock.now();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let names = [
        "shots:first",
        "shots:second",
        "shots:third",
        "lint:aggregate",
    ];
    for name in names {
        list_check(&mut conn, name);
        let hash = if name.starts_with("shots:") {
            "abcd"
        } else {
            ""
        };
        imported_flake(&conn, name, hash, None, now);
    }
    let report = upkeep.pass(&mut conn, now).unwrap();
    assert!(report.tickets_filed.is_empty());
    assert!(report.checks_switched_off.is_empty());

    checks::record_flaky(&conn, &flaky("shots:first", "bbbb", "m-1:run-1"), now).unwrap();
    let report = upkeep.pass(&mut conn, now).unwrap();
    assert_eq!(report.checks_switched_off, ["shots:first"]);
    assert_eq!(report.tickets_filed.len(), 1);
    let evidence = checks::filed_flakes(&conn, &report.tickets_filed[0]).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].command, "m-1:run-1");
    let history = hub::check_state::flaky_checks(&conn).unwrap();
    assert_eq!(history.len(), names.len());
    for entry in history {
        if entry.check.as_str() == "shots:first" {
            assert_eq!((entry.state, entry.flaky_records), (CheckState::Off, 2));
        } else {
            assert_eq!((entry.state, entry.flaky_records), (CheckState::On, 1));
        }
    }
}

#[test]
fn recovery_restores_only_checks_disabled_by_the_import_and_runs_once() {
    let clock = start();
    let now = clock.now();
    let mut conn = hub::open_in_memory().unwrap();
    let names = [
        "shots:imported",
        "shots:hashless",
        "shots:native",
        "shots:native-without-command",
        "shots:test-only",
        "shots:pending-test-only",
        "shots:manual",
        "shots:unfiled",
        "shots:unlinked",
    ];
    let family_ticket = "FLA01";
    let batch_ticket = "FLA02";
    imported_remedy(
        &conn,
        family_ticket,
        "flaky",
        &names[..names.len() - 1],
        now,
    );
    imported_remedy(
        &conn,
        batch_ticket,
        hub::flaky_triage::CATEGORY,
        &names[1..2],
        now,
    );
    for name in names {
        list_check(&mut conn, name);
        let filed = match name {
            "shots:unfiled" => None,
            "shots:hashless" => Some(batch_ticket),
            _ => Some(family_ticket),
        };
        let hash = if name == "shots:hashless" { "" } else { "abcd" };
        imported_flake(&conn, name, hash, filed, now);
        if name == "shots:manual" {
            hub::check_state::switch(
                &conn,
                &hub::check_state::Switch {
                    check: name,
                    state: CheckState::Off,
                    actor: "operator",
                    machine: "m-1",
                    note: "Disabled by the operator",
                    ticket: None,
                    at: now,
                },
            )
            .unwrap();
        } else {
            checks::set_state(&conn, name, CheckState::Off).unwrap();
        }
    }
    for (name, command) in [
        ("shots:native", "m-1:run-1"),
        ("shots:native-without-command", ""),
    ] {
        checks::record_flaky(&conn, &flaky(name, "bbbb", command), now).unwrap();
    }
    for (name, remedy) in [
        ("shots:test-only", "TEST1"),
        ("shots:pending-test-only", ""),
    ] {
        conn.execute(
            "INSERT INTO check_fix_outcomes
                (close_seq, check_name, ticket, at, outcome, source, first_bad, fixing_commit, remedy_ticket)
             VALUES (1, ?1, 'REG01', ?2, 'test-only', 'regression', 'aaaa', 'bbbb', ?3)",
            params![name, now.to_string(), remedy],
        )
        .unwrap();
    }

    let report = Upkeep::new(None).unwrap().pass(&mut conn, now).unwrap();
    assert_eq!(report.checks_restored, ["shots:hashless", "shots:imported"]);
    for name in &names[2..] {
        assert_eq!(check_state(&conn, name), "off", "{name}");
    }
    let history = hub::check_state::flaky_checks(&conn).unwrap();
    for (name, prior_ticket) in names[..2].iter().zip([family_ticket, batch_ticket]) {
        let entry = history
            .iter()
            .find(|entry| entry.check.as_str() == *name)
            .unwrap();
        assert_eq!((entry.state, entry.flaky_records), (CheckState::On, 1));
        assert_eq!(entry.ticket.as_deref(), Some(prior_ticket));
        let switch = entry.switched.as_ref().unwrap();
        assert_eq!((switch.actor.as_str(), switch.at), ("ignite-upkeep", now));
    }

    // A later pass on a restarted hub makes no new changes.
    clock.advance(60.0);
    let mut restarted = Upkeep::new(None).unwrap();
    let report = restarted.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_restored.is_empty());
    assert_eq!(hub::check_state::flaky_checks(&conn).unwrap(), history);
}

#[test]
fn twenty_ordinary_check_families_share_one_pending_batch() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    hold_sources(&mut conn);
    let mut upkeep = Upkeep::new(None).unwrap();
    let mut names: Vec<_> = (0..19).map(|n| format!("family-{n:02}:check")).collect();
    names.push("ui-smoke:protected".to_owned());
    for (n, name) in names.iter().enumerate() {
        list_check(&mut conn, name);
        let command = if n < 2 {
            "m-1:shared".to_owned()
        } else {
            format!("m-1:{n}")
        };
        checks::record_flaky(&conn, &flaky(name, "aaaa", &command), clock.now()).unwrap();
    }

    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.checks_switched_off, names[..19]);
    assert_eq!(report.tickets_filed.len(), 1);
    let shared = ticket(&conn, &report.tickets_filed[0]);
    assert_eq!(shared.status, Status::PendingApproval);
    assert_eq!(shared.priority, 3);
    assert_eq!(shared.metadata["ignite_checks"], serde_json::json!(names));
    assert_eq!(shared.body.matches("- Command ").count(), 19);
    assert!(
        shared
            .body
            .contains("Checks: family-00:check, family-01:check.")
    );
    assert_eq!(check_state(&conn, "family-00:check"), "off");
    assert_eq!(check_state(&conn, "ui-smoke:protected"), "on");
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
    let evidence = checks::filed_flakes(&conn, shared.id.as_str()).unwrap();
    assert_eq!(evidence.len(), 20);
    assert_eq!(evidence[1].output, "family-01:check failed in m-1:shared");
    assert_eq!(
        ignite_store::hub::check_state::flaky_ticket(&conn, "family-00:check").unwrap(),
        Some(shared.id.to_string())
    );

    checks::set_state(
        &conn,
        "family-00:check",
        ignite_core::protocol::hub::CheckState::On,
    )
    .unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_switched_off.is_empty() && report.tickets_filed.is_empty());
    assert_eq!(check_state(&conn, "family-00:check"), "on");
}

#[test]
fn a_pending_batch_updates_its_cluster_summary_until_approval() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    hold_sources(&mut conn);
    let mut upkeep = Upkeep::new(None).unwrap();
    let mut seed = |conn: &mut Connection, name: &str, command: &str| {
        list_check(conn, name);
        checks::record_flaky(conn, &flaky(name, "aaaa", command), clock.now()).unwrap();
        let pass = upkeep.pass(conn, clock.now()).unwrap();
        let [id]: [String; 1] = pass.tickets_filed.try_into().expect("one filing");
        id
    };
    let first = seed(&mut conn, "ui:shape", "m-1:shared");
    list_check(&mut conn, "unit:legacy");
    imported_flake(&conn, "unit:legacy", "", None, clock.now());
    assert_eq!(first, seed(&mut conn, "ui:shot", "m-1:shared"));
    let frozen = ticket(&conn, &first);
    assert_eq!(frozen.body.matches("- Command ").count(), 1);
    assert_eq!(checks::filed_flakes(&conn, &first).unwrap().len(), 2);
    assert_eq!(check_state(&conn, "unit:legacy"), "on");

    for status in [Status::Open, Status::PendingApproval] {
        apply(
            &mut conn,
            vec![Operation::Update {
                id: first.clone(),
                set: Some(TicketFields {
                    status: Some(status),
                    ..TicketFields::default()
                }),
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: BTreeMap::new(),
                unset_meta: Vec::new(),
            }],
            clock.now(),
        );
        if status == Status::Open {
            clock.advance(1.0);
            assert_ne!(first, seed(&mut conn, "unit:next", "m-1:new"));
        }
    }
    clock.advance(1.0);
    let next = seed(&mut conn, "unit:later", "m-1:later");
    assert_ne!(first, next);
    assert_eq!(ticket(&conn, &first).body, frozen.body);
    assert_eq!(ticket(&conn, &first).metadata, frozen.metadata);
    assert_eq!(checks::filed_flakes(&conn, &next).unwrap().len(), 2);
    assert_eq!(all_tickets(&conn, clock.now()).len(), 2);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
}

fn store(conn: &mut Connection, machine: &str, outcome: Outcome, now: Timestamp) -> u16 {
    let request = StoreRequest {
        results: vec![NewResult {
            key: ResultKey {
                check: CheckName::new("rust:x").unwrap(),
                hash: InputHash::new("2".repeat(64)).unwrap(),
                platform: None,
            },
            outcome,
            machine: MachineId::new(machine).unwrap(),
            record: RecordId(0),
            output: (outcome == Outcome::Fail).then(|| "/outputs/run-7.log".to_owned()),
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        }],
        areas: Vec::new(),
    };
    let request = Request {
        method: "POST".to_owned(),
        path: "/results/store".to_owned(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: MachineId::new(machine).unwrap(),
            actor: "daemon".to_owned(),
        }),
        body: serde_json::to_vec(&request).unwrap(),
        headers: Vec::new(),
    };
    check_routes::handle(conn, &request, now).unwrap().status
}

#[test]
fn a_pass_that_replaces_another_machines_failure_files_one_p1_for_that_machine() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    register(&conn, "m-1", "cygnus", Os::Windows, clock.now());
    register(&conn, "m-2", "jarvis", Os::Macos, clock.now());
    list_check(&mut conn, "rust:x");

    assert_eq!(store(&mut conn, "m-1", Outcome::Fail, clock.now()), 200);
    assert!(all_tickets(&conn, clock.now()).is_empty());
    clock.advance(60.0);
    assert_eq!(store(&mut conn, "m-2", Outcome::Pass, clock.now()), 200);

    let key = ResultKey {
        check: CheckName::new("rust:x").unwrap(),
        hash: InputHash::new("2".repeat(64)).unwrap(),
        platform: None,
    };
    let cached = results::lookup(&conn, &[key], clock.now()).unwrap();
    let cached = cached[0].as_ref().unwrap();
    assert_eq!(
        (cached.outcome, cached.machine.as_str()),
        (Outcome::Pass, "m-2")
    );
    let filed = all_tickets(&conn, clock.now());
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].priority, 1);
    assert!(filed[0].labels.contains(&"machine:cygnus".to_owned()));
    assert!(filed[0].body.contains("/outputs/run-7.log"));

    // A pass after a pass, or after the same machine's own failure, files nothing.
    clock.advance(60.0);
    store(&mut conn, "m-1", Outcome::Pass, clock.now());
    store(&mut conn, "m-1", Outcome::Fail, clock.now());
    store(&mut conn, "m-1", Outcome::Pass, clock.now());
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
}

#[test]
fn unknown_machine_and_provider_pins_are_reported() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let pinned = |title: &str, labels: &[&str]| Operation::Create {
        fields: TicketFields {
            title: Some(title.to_owned()),
            ticket_type: Some(TicketType::Task),
            labels: Some(labels.iter().map(|l| (*l).to_owned()).collect()),
            ..TicketFields::default()
        },
        parent: None,
        deps: Vec::new(),
        request_key: None,
    };
    let ids = apply(
        &mut conn,
        vec![
            pinned("On an unregistered machine", &["machine:plan9"]),
            pinned(
                "With an unknown provider",
                &["machine:jarvis", "provider:gemini"],
            ),
            pinned("On jarvis", &["machine:jarvis", "provider:claude"]),
            pinned("On any provider of jarvis", &["machine:jarvis"]),
        ],
        clock.now(),
    );
    register(&conn, "m-1", "jarvis", Os::Macos, clock.now());
    conn.execute("UPDATE machines SET cli_apps = ?1", [r#"["claude"]"#])
        .unwrap();
    // Listed by ticket ID, which is random, so each is found by its ID.
    let stranded = machines::unassignable(&conn).unwrap();
    assert_eq!(stranded.len(), 2, "{stranded:?}");
    let reason = |id: &str| &stranded.iter().find(|(t, _)| t == id).unwrap().1;
    assert!(reason(&ids[0]).contains("plan9"));
    assert!(reason(&ids[1]).contains("gemini"));
}
