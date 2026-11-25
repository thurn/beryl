//! Upkeep against an in-memory hub database, with time from a fake clock.

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    AuditReport, CheckState, FoundBy, IncompleteReason, IncompleteReport, MachineRegister,
    NewResult, RegressionReport, ResultKey, TipWaiting,
};
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, EdgeKind, Operation, Status, Ticket, TicketFields, TicketQuery,
    TicketType,
};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::Clock;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{self, checks, edges, fleet, results, rows, tickets, tips};

use super::filing::{Change, UPKEEP};
use super::{Upkeep, backup, regressions, stuck};

/// Sets every automated source's allocation to zero, so the tickets the hub files stay pending
/// for review instead of opening on the next pass ([`super::source_release`]).
pub(super) fn hold_sources(conn: &mut Connection) {
    let mut settings = ignite_store::hub::fleet_settings::settings(conn).unwrap();
    let version = settings.version;
    settings.allocations = Some(ignite_core::fleet::Allocations {
        general: None,
        sources: ignite_core::shares::Source::ALL
            .into_iter()
            .map(|source| (source, 0))
            .collect(),
    });
    assert!(matches!(
        ignite_store::hub::fleet_settings::put(conn, version, settings, "operator", start().now())
            .unwrap(),
        ignite_store::hub::fleet_settings::PutOutcome::Stored(_)
    ));
}

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
        search: None,
        found_by: FoundBy::TipVerification,
        detected_at: None,
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
    // Ignite filed the regression on its own, so its ID starts with `A`; the culprit a session
    // filed keeps its `T`.
    assert!(
        first.starts_with('A') && culprit[0].starts_with('T'),
        "{first} {}",
        culprit[0]
    );

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

fn failing(check: &str, first_bad: &str) -> RegressionReport {
    let mut report = regression(first_bad, "assertion failed");
    report.check = CheckName::new(check).unwrap();
    report
}

fn file(
    conn: &mut Connection,
    clock: &FakeClock,
    report: &RegressionReport,
) -> (String, regressions::Outcome) {
    clock.advance(60.0);
    regressions::file(conn, UPKEEP, report, clock.now()).unwrap()
}

fn regression_ticket_of(conn: &Connection, check: &str) -> String {
    conn.query_row(
        "SELECT ticket FROM regressions WHERE check_name = ?1",
        [check],
        |row| row.get(0),
    )
    .unwrap()
}

/// A worker takes the ticket.
fn hold(conn: &Connection, id: &str) {
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = 'local_w', session = 'local_w', \
         tree = 'tree-11', machine = 'jarvis' WHERE id = ?1",
        [id],
    )
    .unwrap();
}

#[test]
fn a_gate_search_names_the_culprit_on_a_regression_already_claimed() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let culprit = apply(
        &mut conn,
        vec![create("Break the check", TicketType::Task, None, &[])],
        clock.now(),
    );
    let mut report = regression("2222", "assertion failed");
    report.found_by = FoundBy::Landing;
    let (id, _) = file(&mut conn, &clock, &report);
    hold(&conn, &id);

    report.caused_by = Some(TicketId::new(&culprit[0]).unwrap());
    report.detected_at = Some(report.first_bad);
    report.first_bad = Commit::new("1111").unwrap();
    report.search = Some(CulpritSearch::Bisected { runs: 1 });
    assert_eq!(file(&mut conn, &clock, &report).0, id);

    let current = ticket(&conn, &id);
    assert!(
        current
            .body
            .starts_with(&format!("caused-by  {} ", culprit[0]))
    );
    let comments = tickets::comments(&conn, &id).unwrap();
    let [comment] = comments.as_slice() else {
        panic!("expected one comment, got {}", comments.len());
    };
    assert_eq!(current.metadata["tell"], serde_json::json!(comment.n));
    assert!(
        comment
            .text
            .contains(&format!("caused-by  {} ", culprit[0]))
    );
    assert_eq!(current.assignee.as_deref(), Some("local_w"));

    assert_eq!(file(&mut conn, &clock, &report).0, id);
    assert_eq!(comment_count(&conn, &id), 1);
}

#[test]
fn failing_checks_share_a_ticket_unless_another_landing_finds_every_ticket_worked() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let (a, outcome) = file(&mut conn, &clock, &failing("rust:a::one", "1111"));
    assert_eq!(outcome, regressions::Outcome::Created);
    // Another check from the same landing, and one from another landing while nobody has taken
    // the ticket yet, both join it.
    for (check, sha) in [("pytest:t.py::two", "1111"), ("rust:a::three", "2222")] {
        let (joined, outcome) = file(&mut conn, &clock, &failing(check, sha));
        assert_eq!(
            (joined.as_str(), outcome),
            (a.as_str(), regressions::Outcome::Added)
        );
    }
    let joined = ticket(&conn, &a);
    assert!(joined.labels.contains(&"check:pytest".to_owned()));
    assert!(joined.metadata.contains_key(regressions::ADDED_KEY));
    let comments = tickets::comments(&conn, &a).unwrap();
    assert!(
        comments[0]
            .text
            .contains("Another check fails: pytest:t.py::two")
    );
    // Once a worker holds it, a check from a landing already on it still joins it...
    hold(&conn, &a);
    let (joined, _) = file(&mut conn, &clock, &failing("rust:a::four", "2222"));
    assert_eq!(joined, a);
    assert_eq!(regression_ticket_of(&conn, "rust:a::four"), a);
    // ...but one from a new landing gets a ticket of its own, up to three.
    let (b, outcome) = file(&mut conn, &clock, &failing("rust:a::five", "5555"));
    assert_eq!(outcome, regressions::Outcome::Created);
    hold(&conn, &b);
    let (c, _) = file(&mut conn, &clock, &failing("rust:a::six", "6666"));
    hold(&conn, &c);
    assert_eq!(all_tickets(&conn, clock.now()).len(), 3);
    // With three open, the next joins the ticket with the fewest checks, the first of a tie.
    let (fewest, outcome) = file(&mut conn, &clock, &failing("rust:a::seven", "7777"));
    assert_eq!(outcome, regressions::Outcome::Added);
    assert_eq!(fewest, [&b, &c].into_iter().min().unwrap().clone());
    assert_eq!(all_tickets(&conn, clock.now()).len(), 3);
}

#[test]
fn a_closed_check_failing_again_joins_an_open_ticket_instead_of_reopening() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let (old, _) = file(&mut conn, &clock, &failing("rust:a::one", "1111"));
    close(&mut conn, &old, clock.now());
    let (open, _) = file(&mut conn, &clock, &failing("rust:a::two", "2222"));
    let (moved, outcome) = file(&mut conn, &clock, &failing("rust:a::one", "9999"));
    assert_eq!(
        (moved.as_str(), outcome),
        (open.as_str(), regressions::Outcome::Added)
    );
    assert_eq!(ticket(&conn, &old).status, Status::Closed);
    assert_eq!(regression_ticket_of(&conn, "rust:a::one"), open);
    let state: String = conn
        .query_row(
            "SELECT state FROM regressions WHERE check_name = 'rust:a::one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, tips::REOPENED);
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
    report.search = Some(CulpritSearch::EndedEarly {
        runs: 1,
        candidates: 2,
        why: "a run at 0000aaa was incomplete".to_owned(),
    });
    let (id, _) = regressions::file(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    let filed = ticket(&conn, &id);
    assert_eq!(filed.needs_os, Some(Os::Macos));
    assert!(filed.fallback);
    assert!(!filed.labels.iter().any(|label| label.starts_with("needs:")));
    assert!(filed.body.contains("failed on macos"));
    // The ticket says the range is a range, so its first bad commit is not read as the culprit.
    assert!(
        filed.body.contains(
            "culprit: the search ended early after 1 run, with 2 candidate landings, because a \
             run at 0000aaa was incomplete"
        ),
        "{}",
        filed.body
    );
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
    // A worker held the ticket when it closed it, and a closed ticket keeps its assignee.
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = 'local_w', session = 'local_w', \
         tree = 'tree-11', machine = 'jarvis' WHERE id = ?1",
        [&id],
    )
    .unwrap();
    close(&mut conn, &id, clock.now());
    assert_eq!(ticket(&conn, &id).assignee.as_deref(), Some("local_w"));

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
    // Dispatch offers only tickets nobody holds, so the reopened one has no assignee.
    assert_eq!(ticket(&conn, &id).assignee, None);
    // Nor the old claim's session and tree: a daemon that still saw them took the ticket for
    // held by that session and refused every other worker's `ignite start`.
    let reopened = ticket(&conn, &id);
    assert_eq!(
        (reopened.session, reopened.tree, reopened.machine),
        (None, None, None)
    );
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
    let inside = apply(
        &mut conn,
        vec![create("Inside", TicketType::Task, Some(&epic[0]), &[])],
        clock.now(),
    );
    // Upkeep still handles external dependencies, such as one the user approved with
    // `dep add --external` or one saved before the batch began refusing them.
    edges::add(&conn, &inside[0], &outside[0], EdgeKind::Blocks).unwrap();
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
        checks: Vec::new(),
        count: 0,
        product: None,
        holds_tip: false,
    };
    let filed = stuck::incomplete(&mut conn, UPKEEP, &incomplete, clock.now()).unwrap();
    let held = ticket(&conn, &filed.ticket);
    assert_eq!(held.priority, 1);
    assert!(held.title.contains("multivac"));
    assert_eq!(held.needs_os, Some(Os::Macos));
    assert!(held.labels.contains(&"machine:multivac".to_owned()));

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

/// The tickets in `filed` other than those [`super::silent`] files for a silent machine, which
/// tests of other faults set up without meaning to.
pub fn except_silent(conn: &Connection, filed: &[String]) -> Vec<String> {
    filed
        .iter()
        .filter(|id| !ticket(conn, id).title.ends_with("has stopped reporting"))
        .cloned()
        .collect()
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
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(except_silent(&conn, &report.tickets_filed).is_empty());

    clock.advance(1_800.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    let waiting = except_silent(&conn, &report.tickets_filed);
    assert_eq!(waiting.len(), 1);
    let filed = ticket(&conn, &waiting[0]);
    assert!(filed.title.contains("multivac") && filed.body.contains("abcd"));
    assert!(filed.labels.contains(&"machine:multivac".to_owned()));
    assert_eq!(filed.priority, 1);
    clock.advance(600.0);
    tips::seen(&conn, "ef01", clock.now()).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(except_silent(&conn, &report.tickets_filed).is_empty());
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
        unrunnable: Vec::new(),
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
    assert_eq!(named.needs_os, Some(Os::Windows));
    assert!(named.labels.contains(&"machine:cygnus".to_owned()));

    // A later tip waiting on the same machine adds to its ticket.
    clock.advance(600.0);
    let again = stuck::tip_waiting(&mut conn, UPKEEP, &waiting("ef01"), clock.now()).unwrap();
    assert_eq!(again[0].ticket, filed[0].ticket);
    assert_ne!(again[0].change, Change::Created);

    // Once cygnus stores a result it is running the tip's checks, so a later tip that waits on
    // it files nothing.
    let result = NewResult {
        key: ResultKey {
            check: CheckName::new("rust:windows_paths").unwrap(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform: Some(Os::Windows),
        },
        outcome: Outcome::Pass,
        machine: MachineId::new("m-3").unwrap(),
        record: RecordId(1),
        output: None,
        cost: ignite_core::protocol::hub::ResultCost::default(),
        areas: Vec::new(),
    };
    clock.advance(60.0);
    results::store(&mut conn, &[result], clock.now()).unwrap();
    let busy = stuck::tip_waiting(&mut conn, UPKEEP, &waiting("ab12"), clock.now()).unwrap();
    assert!(busy.is_empty(), "{busy:?}");
}

#[test]
fn backups_are_hourly_and_exports_daily() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-upkeep-backup-{}", std::process::id()));
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
    let archive = snapshot.join(backup::ARCHIVE);
    let database = snapshot.join(backup::DATABASE);
    assert!(!database.exists());
    let restored = zstd::stream::decode_all(std::fs::File::open(&archive).unwrap()).unwrap();
    std::fs::write(&database, &restored).unwrap();
    let copy = Connection::open(&database).unwrap();
    let count: i64 = copy
        .query_row("SELECT count(*) FROM tickets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    // Windows removes no snapshot that a connection holds open, and the passes below prune them.
    drop(copy);
    let manifest = std::fs::read_to_string(snapshot.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"sha256\""));

    // A snapshot from before compression is upgraded even when no new backup is due.
    std::fs::remove_file(&archive).unwrap();
    clock.advance(1_800.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert!(report.snapshot.is_none() && report.export.is_none());
    assert!(!database.exists());
    assert_eq!(
        zstd::stream::decode_all(std::fs::File::open(archive).unwrap()).unwrap(),
        restored
    );
    assert_eq!(
        std::fs::read_to_string(snapshot.join("manifest.json")).unwrap(),
        manifest
    );

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

/// A regression ticket that ended with an "output" heading and nothing under it left workers
/// unable to tell which count had failed. With no output, the ticket says so in a sentence.
#[test]
fn a_regression_without_output_says_so_and_has_no_empty_heading() {
    let text = super::regressions::details(&regression("abcd", "  \n"));

    assert!(text.contains("no output: the failing run reported none"));
    assert!(!text.contains("output (last"));

    let with = super::regressions::details(&regression("abcd", "boom"));
    assert!(with.ends_with("output (last 40 lines):\nboom"));
}
