//! The flaky-check guards, machines that disagree, and tickets no machine can take, against an
//! in-memory hub database with time from a fake clock.

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    FlakyReport, MachineRegister, NewResult, ResultKey, StoreRequest,
};
use ignite_core::protocol::tickets::{Operation, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub::{self, checks, fleet, results};

use super::tests::{all_tickets, apply, check_state, comment_count, list_check, start, ticket};
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
fn an_always_set_check_that_flakes_twice_stays_on_and_gets_one_p1() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    list_check(&mut conn, "format:rust");
    let always = |commit: &str| FlakyReport {
        always: true,
        ..flaky("format:rust", commit, &format!("m-1:{commit}"))
    };

    checks::record_flaky(&conn, &always("aaaa"), clock.now()).unwrap();
    clock.advance(86_400.0);
    checks::record_flaky(&conn, &always("bbbb"), clock.now()).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_switched_off.is_empty());
    assert_eq!(check_state(&conn, "format:rust"), "on");
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert_eq!(filed.priority, 1);
    assert!(filed.body.contains("format:rust failed in m-1:aaaa"));
    assert!(filed.body.contains("format:rust failed in m-1:bbbb"));

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
    assert!(machine.labels.contains(&"needs:jarvis".to_owned()));
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
    assert!(report.checks_switched_off.is_empty() && report.tickets_filed.is_empty());
    for name in names {
        assert_eq!(check_state(&conn, name), "on");
    }
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
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
    assert!(filed[0].labels.contains(&"needs:cygnus".to_owned()));
    assert!(filed[0].body.contains("/outputs/run-7.log"));

    // A pass after a pass, or after the same machine's own failure, files nothing.
    clock.advance(60.0);
    store(&mut conn, "m-1", Outcome::Pass, clock.now());
    store(&mut conn, "m-1", Outcome::Fail, clock.now());
    store(&mut conn, "m-1", Outcome::Pass, clock.now());
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
}

#[test]
fn a_ticket_needing_a_system_no_machine_runs_is_unassignable() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    register(&conn, "m-1", "jarvis", Os::Macos, clock.now());
    let with = |title: &str, label: &str| Operation::Create {
        fields: TicketFields {
            title: Some(title.to_owned()),
            ticket_type: Some(TicketType::Task),
            labels: Some(vec![label.to_owned()]),
            ..TicketFields::default()
        },
        parent: None,
        deps: Vec::new(),
        request_key: None,
    };
    let ids = apply(
        &mut conn,
        vec![
            with("Windows only", "needs:windows"),
            with("Mac only", "needs:macos"),
            with("On jarvis", "needs:jarvis"),
            with("Ask a person", "needs:person"),
        ],
        clock.now(),
    );

    let stranded = machines::unassignable(&conn).unwrap();
    assert_eq!(stranded.len(), 1);
    assert_eq!(stranded[0].0, ids[0]);
    assert!(stranded[0].1.contains("needs:windows"));

    // A machine that runs the system makes the ticket assignable.
    register(&conn, "m-2", "cygnus", Os::Windows, clock.now());
    assert!(machines::unassignable(&conn).unwrap().is_empty());
}
