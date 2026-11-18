//! Upkeep against an in-memory hub database, with time from a fake clock.

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    AuditReport, CheckState, ExplorationFind, FlakyReport, IncompleteReason, IncompleteReport,
    MachineRegister, NewResult, RegressionReport, ResultKey, TipWaiting,
};
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, Operation, Status, Ticket, TicketFields, TicketQuery, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{self, checks, fleet, results, rows, tickets, tips};

use super::filing::{Change, UPKEEP};
use super::{Upkeep, backup, explore, regressions, stuck};

pub(super) fn start() -> FakeClock {
    FakeClock::new(Timestamp::parse("2026-11-17T12:00:00Z").unwrap())
}

pub(super) fn apply(conn: &mut Connection, ops: Vec<Operation>, now: Timestamp) -> Vec<String> {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply(conn, &request, "test", "m-1", now).unwrap() {
        BatchOutcome::Applied(reply) => reply.results.iter().map(|r| r.id.to_string()).collect(),
        BatchOutcome::Conflict(conflict) => panic!("conflict at {}", conflict.failed),
    }
}

pub(super) fn create(
    title: &str,
    ticket_type: TicketType,
    parent: Option<&str>,
    deps: &[&str],
) -> Operation {
    Operation::Create {
        fields: TicketFields {
            title: Some(title.to_owned()),
            ticket_type: Some(ticket_type),
            ..TicketFields::default()
        },
        parent: parent.map(str::to_owned),
        deps: deps
            .iter()
            .map(|on| Dependency {
                on: (*on).to_owned(),
                kind: None,
            })
            .collect(),
        request_key: None,
    }
}

fn close(conn: &mut Connection, id: &str, now: Timestamp) {
    let op = Operation::Close {
        id: id.to_owned(),
        reason: "fixed".to_owned(),
        include_ready: false,
    };
    apply(conn, vec![op], now);
}

pub(super) fn all_tickets(conn: &Connection, now: Timestamp) -> Vec<Ticket> {
    let query = TicketQuery {
        all: true,
        ..TicketQuery::default()
    };
    tickets::list(conn, &query, now).unwrap()
}

pub(super) fn ticket(conn: &Connection, id: &str) -> Ticket {
    tickets::load(conn, id).unwrap().unwrap()
}

pub(super) fn comment_count(conn: &Connection, id: &str) -> usize {
    tickets::comments(conn, id).unwrap().len()
}

fn regression(first_bad: &str, output: &str) -> RegressionReport {
    RegressionReport {
        check: CheckName::new("rust:a::b").unwrap(),
        first_bad: Commit::new(first_bad).unwrap(),
        last_good: Commit::new("0000").unwrap(),
        caused_by: None,
        output: output.to_owned(),
        suspects: Vec::new(),
        platform: None,
    }
}

pub(super) fn list_check(conn: &mut Connection, name: &str) {
    let check = rows::Check {
        name: name.to_owned(),
        family: name.split(':').next().unwrap().to_owned(),
        kind: "regression".to_owned(),
        state: "on".to_owned(),
        check_group: "group".to_owned(),
    };
    checks::upsert(conn, &[check]).unwrap();
}

pub(super) fn check_state(conn: &Connection, name: &str) -> String {
    conn.query_row("SELECT state FROM checks WHERE name = ?1", [name], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn two_failures_of_one_check_give_one_regression_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let culprit = apply(
        &mut conn,
        vec![create("Ward counts", TicketType::Task, None, &[])],
        clock.now(),
    );
    let mut report = regression("1111", "assertion failed");
    report.caused_by = Some(TicketId::new(&culprit[0]).unwrap());
    let (first, outcome) = regressions::file(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    assert_eq!(outcome, regressions::Outcome::Created);

    clock.advance(600.0);
    let (second, outcome) =
        regressions::file(&mut conn, UPKEEP, &regression("2222", "again"), clock.now()).unwrap();
    assert_eq!(
        (second.as_str(), outcome),
        (first.as_str(), regressions::Outcome::Unchanged)
    );

    let filed = ticket(&conn, &first);
    assert_eq!(filed.priority, 0);
    assert_eq!(filed.ticket_type, TicketType::Bug);
    assert_eq!(filed.labels, vec!["regression", "check:rust"]);
    assert!(
        filed
            .body
            .contains(&format!("caused-by  {} (commit 1111)", culprit[0]))
    );
    assert!(filed.body.contains("first bad 1111, last good 0000"));
    assert_eq!(all_tickets(&conn, clock.now()).len(), 2);
}

#[test]
fn a_range_links_every_suspect_and_a_platform_check_needs_its_system() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let suspects = apply(
        &mut conn,
        vec![
            create("One", TicketType::Task, None, &[]),
            create("Two", TicketType::Task, None, &[]),
        ],
        clock.now(),
    );
    let mut report = regression("1111", "boom");
    report.suspects = suspects
        .iter()
        .map(|id| TicketId::new(id).unwrap())
        .collect();
    report.platform = Some(Os::Macos);
    let (id, _) = regressions::file(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    let filed = ticket(&conn, &id);
    assert!(filed.labels.contains(&"needs:macos".to_owned()));
    assert!(filed.body.contains("failed on macos"));
    let linked: Vec<String> = conn
        .prepare("SELECT dst FROM edges WHERE src = ?1 ORDER BY dst")
        .unwrap()
        .query_map([&id], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut expected = suspects.clone();
    expected.sort();
    assert_eq!(linked, expected);
}

#[test]
fn a_fix_that_did_not_work_reopens_the_ticket_with_the_new_output() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let (id, _) = regressions::file(
        &mut conn,
        UPKEEP,
        &regression("1111", "left: 3"),
        clock.now(),
    )
    .unwrap();
    clock.advance(3_600.0);
    close(&mut conn, &id, clock.now());

    clock.advance(600.0);
    let (again, outcome) = regressions::file(
        &mut conn,
        UPKEEP,
        &regression("3333", "left: 5"),
        clock.now(),
    )
    .unwrap();
    assert_eq!(
        (again.as_str(), outcome),
        (id.as_str(), regressions::Outcome::Reopened)
    );
    assert_eq!(ticket(&conn, &id).status, Status::Open);
    let comments = tickets::comments(&conn, &id).unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].text.contains("still fails"));
    assert!(comments[0].text.contains("left: 5"));
    let (state, first_bad): (String, String) = conn
        .query_row("SELECT state, first_bad FROM regressions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(
        (state.as_str(), first_bad.as_str()),
        (tips::REOPENED, "3333")
    );
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
}

#[test]
fn a_pass_after_the_fix_closed_marks_the_regression_fixed() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let (id, _) =
        regressions::file(&mut conn, UPKEEP, &regression("1111", "x"), clock.now()).unwrap();
    let pass = |conn: &mut Connection, n: char, now: Timestamp| {
        let result = NewResult {
            key: ResultKey {
                check: CheckName::new("rust:a::b").unwrap(),
                hash: InputHash::new(n.to_string().repeat(64)).unwrap(),
                platform: None,
            },
            outcome: Outcome::Pass,
            machine: MachineId::new("m-1").unwrap(),
            record: RecordId(1),
            output: None,
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        };
        results::store(conn, &[result], now).unwrap();
    };
    // A pass before the ticket closed says nothing about the fix.
    clock.advance(60.0);
    pass(&mut conn, '1', clock.now());
    clock.advance(60.0);
    close(&mut conn, &id, clock.now());
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .regressions_fixed
            .is_empty()
    );

    clock.advance(60.0);
    pass(&mut conn, '2', clock.now());
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.regressions_fixed, vec!["rust:a::b"]);
}

#[test]
fn a_check_is_switched_off_on_its_second_flaky_record_and_not_its_first() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    list_check(&mut conn, "ui:shots");
    let flaky = |commit: &str| FlakyReport {
        check: CheckName::new("ui:shots").unwrap(),
        hash: InputHash::new("1".repeat(64)).unwrap(),
        commit: Commit::new(commit).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        always: false,
        command: format!("m-1:{commit}.out"),
        output: format!("timed out waiting at {commit}"),
    };

    checks::record_flaky(&conn, &flaky("aaaa"), clock.now()).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_switched_off.is_empty());
    assert_eq!(check_state(&conn, "ui:shots"), "on");

    clock.advance(2.0 * 86_400.0);
    checks::record_flaky(&conn, &flaky("bbbb"), clock.now()).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.checks_switched_off, vec!["ui:shots"]);
    assert_eq!(check_state(&conn, "ui:shots"), "off");
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert_eq!(filed.priority, 3);
    assert!(filed.body.contains("commit aaaa") && filed.body.contains("commit bbbb"));
    assert!(
        filed.body.contains("timed out waiting at aaaa")
            && filed.body.contains("timed out waiting at bbbb"),
        "the ticket quotes both failures' output"
    );

    // Later passes leave the switched-off check and its ticket alone.
    clock.advance(60.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.checks_switched_off.is_empty() && report.tickets_filed.is_empty());
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);
}

fn find(replay: &str, line: u32, host: &str) -> ExplorationFind {
    ExplorationFind {
        check: CheckName::new(format!("fuzz:seed-{replay}")).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        host: host.to_owned(),
        commit: Commit::new("abcd").unwrap(),
        replay: format!("ignite test fuzz:seed-{replay}"),
        output: format!(
            "thread 'main' (4411) panicked at src/combat.rs:{line}:7:\n\
             assertion failed: blockers <= 2\n on {host}"
        ),
    }
}

#[test]
fn two_finds_with_one_signature_give_one_ticket_and_one_comment() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let first = explore::find(&mut conn, UPKEEP, &find("17", 210, "jarvis"), clock.now()).unwrap();
    assert_eq!(first.change, Change::Created);
    clock.advance(60.0);
    let second =
        explore::find(&mut conn, UPKEEP, &find("94", 214, "multivac"), clock.now()).unwrap();
    assert_eq!(
        (second.ticket.as_str(), second.change),
        (first.ticket.as_str(), Change::Commented)
    );
    // The same find sent again changes nothing.
    let repeat =
        explore::find(&mut conn, UPKEEP, &find("94", 214, "multivac"), clock.now()).unwrap();
    assert_eq!(repeat.change, Change::Unchanged);

    let filed = ticket(&conn, &first.ticket);
    assert_eq!(filed.priority, 2);
    assert_eq!(filed.labels, vec!["found-by:fuzz"]);
    assert!(filed.body.contains("replay: ignite test fuzz:seed-17"));
    assert_eq!(comment_count(&conn, &first.ticket), 1);
    assert_eq!(all_tickets(&conn, clock.now()).len(), 1);

    // A fault that returns after its ticket closed gets a new ticket naming the old one.
    close(&mut conn, &first.ticket, clock.now());
    let returned =
        explore::find(&mut conn, UPKEEP, &find("5", 300, "jarvis"), clock.now()).unwrap();
    assert_eq!(returned.change, Change::Created);
    assert!(ticket(&conn, &returned.ticket).body.contains(&first.ticket));
}

#[test]
fn a_stuck_epic_is_reported_once() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let outside = apply(
        &mut conn,
        vec![create("Outside", TicketType::Task, None, &[])],
        clock.now(),
    );
    let epic = apply(
        &mut conn,
        vec![create("Epic", TicketType::Epic, None, &[])],
        clock.now(),
    );
    apply(
        &mut conn,
        vec![create(
            "Inside",
            TicketType::Task,
            Some(&epic[0]),
            &[&outside[0]],
        )],
        clock.now(),
    );
    // The ticket it waits on is ready, so the epic is not stuck.
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );

    let until = clock.now().plus_seconds(30.0 * 86_400.0);
    apply(
        &mut conn,
        vec![Operation::Defer {
            id: outside[0].clone(),
            until,
        }],
        clock.now(),
    );
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert_eq!(filed.priority, 1);
    assert!(filed.title.contains(&epic[0]));
    assert!(filed.body.contains(&outside[0]));

    clock.advance(600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    assert_eq!(comment_count(&conn, &report.tickets_filed[0]), 0);
}

#[test]
fn an_incomplete_check_and_a_disagreeing_audit_file_p1_tickets() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let incomplete = IncompleteReport {
        check: CheckName::new("ios:smoke").unwrap(),
        machine: MachineId::new("m-2").unwrap(),
        host: "multivac".to_owned(),
        platform: Some(Os::Macos),
        attempts: 3,
        reason: IncompleteReason::CannotRun,
        commit: Commit::new("abcd").unwrap(),
        output: "cannot-run: no simulator on multivac".to_owned(),
    };
    let filed = stuck::incomplete(&mut conn, UPKEEP, &incomplete, clock.now()).unwrap();
    let held = ticket(&conn, &filed.ticket);
    assert_eq!(held.priority, 1);
    assert!(held.title.contains("multivac") && held.labels.contains(&"needs:macos".to_owned()));

    list_check(&mut conn, "rust:x");
    let audit = AuditReport {
        key: ResultKey {
            check: CheckName::new("rust:x").unwrap(),
            hash: InputHash::new("2".repeat(64)).unwrap(),
            platform: None,
        },
        cached: Outcome::Pass,
        cached_record: RecordId(4),
        cached_machine: MachineId::new("m-1").unwrap(),
        rerun: Outcome::Fail,
        rerun_record: RecordId(9),
        rerun_machine: MachineId::new("m-1").unwrap(),
        commit: Commit::new("abcd").unwrap(),
        output: "differs".to_owned(),
    };
    let filed = stuck::audit(&mut conn, UPKEEP, &audit, clock.now()).unwrap();
    assert_eq!(ticket(&conn, &filed.ticket).priority, 1);
    assert!(ticket(&conn, &filed.ticket).body.contains("record 4"));
    assert_eq!(check_state(&conn, "rust:x"), "uncached");
    assert_eq!(
        checks::stats(&conn, "rust").unwrap().checks[0].state,
        CheckState::Uncached
    );
}

#[test]
fn a_tip_that_waits_an_hour_names_the_silent_machine_once() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let register = |name: &str, id: &str| MachineRegister {
        machine: MachineId::new(id).unwrap(),
        name: name.to_owned(),
        os: Os::Macos,
        cores: 16,
        memory_mb: 65_536,
        families: Vec::new(),
        version: "v".to_owned(),
    };
    fleet::register(&conn, &register("jarvis", "m-1"), clock.now()).unwrap();
    fleet::register(&conn, &register("multivac", "m-2"), clock.now()).unwrap();
    clock.advance(60.0);
    tips::seen(&conn, "abcd", clock.now()).unwrap();
    clock.advance(1_800.0);
    fleet::register(&conn, &register("jarvis", "m-1"), clock.now()).unwrap();
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );

    clock.advance(1_800.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(filed.title.contains("multivac") && filed.body.contains("abcd"));
    assert_eq!(filed.priority, 1);
    clock.advance(600.0);
    tips::seen(&conn, "ef01", clock.now()).unwrap();
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
}

#[test]
fn a_tip_waiting_on_another_platform_names_its_machine_on_one_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let register = |name: &str, id: &str, os: Os| MachineRegister {
        machine: MachineId::new(id).unwrap(),
        name: name.to_owned(),
        os,
        cores: 16,
        memory_mb: 65_536,
        families: Vec::new(),
        version: "v".to_owned(),
    };
    let waiting = |commit: &str| TipWaiting {
        commit: Commit::new(commit).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        platform: Os::Windows,
        checks: vec![CheckName::new("rust:windows_paths").unwrap()],
        waited_s: 3_600.0,
    };

    // With no Windows machine registered, the ticket says so.
    let filed = stuck::tip_waiting(&mut conn, UPKEEP, &waiting("abcd"), clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    let none = ticket(&conn, &filed[0].ticket);
    assert!(none.title.contains("No windows machine"));

    fleet::register(&conn, &register("jarvis", "m-1", Os::Macos), clock.now()).unwrap();
    fleet::register(&conn, &register("cygnus", "m-3", Os::Windows), clock.now()).unwrap();
    let filed = stuck::tip_waiting(&mut conn, UPKEEP, &waiting("abcd"), clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    let named = ticket(&conn, &filed[0].ticket);
    assert!(named.title.contains("cygnus"), "{}", named.title);
    assert!(named.body.contains("rust:windows_paths") && named.body.contains("abcd"));
    assert_eq!(named.priority, 1);
    assert!(named.labels.contains(&"needs:windows".to_owned()));

    // A later tip waiting on the same machine adds to its ticket.
    clock.advance(600.0);
    let again = stuck::tip_waiting(&mut conn, UPKEEP, &waiting("ef01"), clock.now()).unwrap();
    assert_eq!(again[0].ticket, filed[0].ticket);
    assert_ne!(again[0].change, Change::Created);
}

#[test]
fn backups_are_hourly_and_exports_daily() {
    let dir = std::env::temp_dir().join(format!("ignite-upkeep-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    apply(
        &mut conn,
        vec![create("Kept", TicketType::Task, None, &[])],
        clock.now(),
    );
    let mut upkeep = Upkeep::new(Some(dir.clone())).unwrap();

    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    let snapshot = report.snapshot.unwrap();
    assert!(report.export.is_some());
    let copy = Connection::open(snapshot.join(backup::DATABASE)).unwrap();
    let count: i64 = copy
        .query_row("SELECT count(*) FROM tickets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let manifest = std::fs::read_to_string(snapshot.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"sha256\""));

    clock.advance(1_800.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.snapshot.is_none() && report.export.is_none());

    // A restarted hub continues from the newest backups on disk.
    let mut upkeep = Upkeep::new(Some(dir.clone())).unwrap();
    clock.advance(1_800.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.snapshot.is_some() && report.export.is_none());

    clock.set(Timestamp::parse("2026-11-18T00:30:00Z").unwrap());
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.export.unwrap().ends_with("exports/2026-11-18.json"));
    assert_eq!(backup::snapshots(&dir).unwrap().len(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
}
