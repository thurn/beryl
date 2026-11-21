//! Tickets the hub filed by itself close once their fault has stayed clear, and only then.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use ignite_core::config::Os;
use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    FoundBy, IncompleteReason, IncompleteReport, MachineRegister, NewResult, RegressionReport,
    ResultCost, ResultKey,
};
use ignite_core::protocol::tickets::{
    BatchRequest, EdgeKind, Operation, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_core::tipround::TipRound;
use ignite_core::traits::Clock;
use ignite_store::hub::batch;
use ignite_store::hub::{self, edges, fleet, results, tickets, tip_rounds, tips};

use super::clearing::{CLOSE_PREFIX, HOLD_SECONDS, RETURNED_HOLD_SECONDS};
use super::filing::{self, Change, FAULT_KEY, FILERS_KEY, Filing, UPKEEP};
use super::signature::Signature;
use super::tests::{apply, comment_count, create, start, ticket};
use super::{Upkeep, regressions, stuck};
use crate::routes::tip_rounds as rounds_route;

fn labels(id: &str, add: &[&str], remove: &[&str]) -> Operation {
    Operation::Update {
        id: id.to_owned(),
        set: None,
        add_labels: add.iter().map(|l| (*l).to_owned()).collect(),
        remove_labels: remove.iter().map(|l| (*l).to_owned()).collect(),
        set_meta: BTreeMap::new(),
        unset_meta: Vec::new(),
    }
}

/// An epic whose one ticket waits for a person, and the graph-repair ticket upkeep filed for it.
fn stuck_epic(conn: &mut Connection, upkeep: &mut Upkeep, clock: &FakeClock) -> (String, String) {
    let epic = apply(
        conn,
        vec![create("Epic", TicketType::Epic, None, &[])],
        clock.now(),
    );
    let inside = apply(
        conn,
        vec![create("Sign", TicketType::Task, Some(&epic[0]), &[])],
        clock.now(),
    );
    apply(
        conn,
        vec![labels(&inside[0], &["needs:person"], &[])],
        clock.now(),
    );
    let report = upkeep.pass(conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    (inside[0].clone(), report.tickets_filed[0].clone())
}

/// Runs passes `step` seconds apart for `seconds`, and returns every ticket they closed.
fn passes(
    conn: &mut Connection,
    upkeep: &mut Upkeep,
    clock: &FakeClock,
    seconds: f64,
) -> Vec<String> {
    let mut closed = Vec::new();
    let step = 600.0;
    let mut waited = 0.0;
    while waited < seconds {
        clock.advance(step);
        waited += step;
        closed.extend(upkeep.pass(conn, clock.now()).unwrap().tickets_closed);
    }
    closed
}

#[test]
fn a_graph_repair_ticket_closes_an_hour_after_its_epic_can_move_and_reopens_if_it_cannot() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let (inside, repair) = stuck_epic(&mut conn, &mut upkeep, &clock);
    assert!(passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).is_empty());

    apply(
        &mut conn,
        vec![labels(&inside, &[], &["needs:person"])],
        clock.now(),
    );
    assert!(passes(&mut conn, &mut upkeep, &clock, HOLD_SECONDS - 600.0).is_empty());
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 1_200.0),
        vec![repair.clone()]
    );
    let closed = ticket(&conn, &repair);
    assert_eq!(closed.status, Status::Closed);
    let reason = closed.close_reason.unwrap();
    assert!(reason.starts_with(CLOSE_PREFIX), "{reason}");
    assert!(reason.contains("can start"), "{reason}");

    // The epic gets stuck again: the same ticket reopens, still waiting for a release.
    clock.advance(600.0);
    apply(
        &mut conn,
        vec![labels(&inside, &["needs:person"], &[])],
        clock.now(),
    );
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed, vec![repair.clone()]);
    let reopened = ticket(&conn, &repair);
    assert_eq!(reopened.status, Status::PendingApproval);
    let comments = tickets::comments(&conn, &repair).unwrap();
    assert!(comments.last().unwrap().text.contains("came back"));

    // A fault that came back once must stay clear for a day.
    apply(
        &mut conn,
        vec![labels(&inside, &[], &["needs:person"])],
        clock.now(),
    );
    assert!(
        passes(
            &mut conn,
            &mut upkeep,
            &clock,
            RETURNED_HOLD_SECONDS - 1_200.0
        )
        .is_empty()
    );
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 1_800.0),
        vec![repair]
    );
}

#[test]
fn a_fault_seen_again_starts_the_hour_over() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let (inside, repair) = stuck_epic(&mut conn, &mut upkeep, &clock);
    apply(
        &mut conn,
        vec![labels(&inside, &[], &["needs:person"])],
        clock.now(),
    );
    assert!(passes(&mut conn, &mut upkeep, &clock, 3_000.0).is_empty());
    apply(
        &mut conn,
        vec![labels(&inside, &["needs:person"], &[])],
        clock.now(),
    );
    assert!(passes(&mut conn, &mut upkeep, &clock, 600.0).is_empty());
    apply(
        &mut conn,
        vec![labels(&inside, &[], &["needs:person"])],
        clock.now(),
    );
    assert!(passes(&mut conn, &mut upkeep, &clock, 3_000.0).is_empty());
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 1_200.0),
        vec![repair]
    );
}

#[test]
fn a_ticket_a_person_commented_on_stays_open_and_a_release_does_not_count() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let (inside, repair) = stuck_epic(&mut conn, &mut upkeep, &clock);
    let release = Operation::Update {
        id: repair.clone(),
        set: Some(TicketFields {
            status: Some(Status::Open),
            ..TicketFields::default()
        }),
        add_labels: Vec::new(),
        remove_labels: Vec::new(),
        set_meta: BTreeMap::new(),
        unset_meta: Vec::new(),
    };
    apply(&mut conn, vec![release], clock.now());
    let (other_inside, other) = stuck_epic(&mut conn, &mut upkeep, &clock);
    let comment = Operation::Comment {
        id: other.clone(),
        text: "I am looking at this.".to_owned(),
    };
    apply(&mut conn, vec![comment], clock.now());

    apply(
        &mut conn,
        vec![
            labels(&inside, &[], &["needs:person"]),
            labels(&other_inside, &[], &["needs:person"]),
        ],
        clock.now(),
    );
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![repair]
    );
    assert_eq!(ticket(&conn, &other).status, Status::PendingApproval);
}

#[test]
fn a_ticket_in_progress_is_told_once_and_left_to_its_worker() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let (inside, repair) = stuck_epic(&mut conn, &mut upkeep, &clock);
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = 's-1', session = 's-1' WHERE id = ?1",
        [&repair],
    )
    .unwrap();
    let before = comment_count(&conn, &repair);
    apply(
        &mut conn,
        vec![labels(&inside, &[], &["needs:person"])],
        clock.now(),
    );
    assert!(passes(&mut conn, &mut upkeep, &clock, 3.0 * HOLD_SECONDS).is_empty());
    assert_eq!(comment_count(&conn, &repair), before + 1);
    let told = tickets::comments(&conn, &repair).unwrap();
    assert!(told.last().unwrap().text.contains("has cleared"));
    assert_eq!(ticket(&conn, &repair).status, Status::InProgress);
}

#[test]
fn a_ticket_filed_before_faults_were_recorded_still_closes() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let (inside, repair) = stuck_epic(&mut conn, &mut upkeep, &clock);
    let strip = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Update {
            id: repair.clone(),
            set: None,
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::new(),
            unset_meta: vec![FAULT_KEY.to_owned(), FILERS_KEY.to_owned()],
        }],
    };
    batch::apply(&mut conn, &strip, UPKEEP.actor, UPKEEP.machine, clock.now()).unwrap();
    apply(
        &mut conn,
        vec![labels(&inside, &[], &["needs:person"])],
        clock.now(),
    );
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![repair.clone()]
    );
    assert!(ticket(&conn, &repair).metadata.contains_key(FAULT_KEY));
}

fn file(conn: &mut Connection, subject: &str, category: &str, now: Timestamp) -> String {
    file_with(
        conn,
        Signature::of_subject(subject, category),
        BTreeMap::new(),
        now,
    )
}

fn file_with(
    conn: &mut Connection,
    signature: Signature,
    metadata: BTreeMap<String, serde_json::Value>,
    now: Timestamp,
) -> String {
    let filing = Filing {
        signature,
        evidence: "first".to_owned(),
        fields: TicketFields {
            title: Some("A fault".to_owned()),
            body: Some("It happened.".to_owned()),
            ticket_type: Some(TicketType::Bug),
            metadata: Some(metadata),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    let filed = filing::file(conn, UPKEEP, &filing, now).unwrap();
    assert_eq!(filed.change, Change::Created);
    filed.ticket
}

#[test]
fn faults_that_cannot_be_seen_to_clear_stay_open() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let flaky = file(&mut conn, "rust:x", "flaky", clock.now());
    let found = file(&mut conn, "scenarios", "found", clock.now());
    assert!(passes(&mut conn, &mut upkeep, &clock, 3.0 * HOLD_SECONDS).is_empty());
    assert_eq!(ticket(&conn, &flaky).status, Status::PendingApproval);
    assert_eq!(ticket(&conn, &found).status, Status::PendingApproval);
}

#[test]
fn a_held_ticket_report_closes_once_the_ticket_is_released() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let work = apply(
        &mut conn,
        vec![create("Work", TicketType::Task, None, &[])],
        clock.now(),
    );
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = 's-1', session = 's-1', claimed_at = ?2
         WHERE id = ?1",
        params![work[0], clock.now().to_string()],
    )
    .unwrap();
    clock.advance(stuck::HELD_SECONDS + 60.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let held = report.tickets_filed[0].clone();
    conn.execute(
        "UPDATE tickets SET status = 'closed' WHERE id = ?1",
        [&work[0]],
    )
    .unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![held.clone()]
    );
    let reason = ticket(&conn, &held).close_reason.unwrap();
    assert!(
        reason.contains(&format!("ticket {} is closed", work[0])),
        "{reason}"
    );
}

fn machine(id: &str, name: &str) -> MachineRegister {
    MachineRegister {
        machine: MachineId::new(id).unwrap(),
        name: name.to_owned(),
        os: Os::Macos,
        cores: 16,
        memory_mb: 65_536,
        families: Vec::new(),
        version: "v".to_owned(),
    }
}

#[test]
fn a_silent_machine_ticket_closes_once_it_reports_and_no_tip_waits() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    fleet::register(&conn, &machine("m-2", "multivac"), clock.now()).unwrap();
    clock.advance(60.0);
    tips::seen(&conn, "abcd", clock.now()).unwrap();
    clock.advance(3_700.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let silent = report.tickets_filed[0].clone();

    // It reports, but the tip still waits.
    clock.advance(60.0);
    fleet::register(&conn, &machine("m-2", "multivac"), clock.now()).unwrap();
    assert!(passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).is_empty());
    conn.execute("UPDATE tips SET state = 'verified', verified = seen", [])
        .unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![silent.clone()]
    );
    assert!(
        ticket(&conn, &silent)
            .close_reason
            .unwrap()
            .contains("multivac reported")
    );
}

#[test]
fn a_tip_wait_ticket_closes_while_its_machine_runs_checks_though_tips_wait() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    fleet::register(&conn, &machine("m-2", "multivac"), clock.now()).unwrap();
    tips::seen(&conn, "abcd", clock.now()).unwrap();
    clock.advance(3_700.0);
    fleet::register(&conn, &machine("m-2", "multivac"), clock.now()).unwrap();
    let waiting = ignite_core::protocol::hub::TipWaiting {
        commit: Commit::new("abcd").unwrap(),
        machine: MachineId::new("m-3").unwrap(),
        platform: Os::Macos,
        checks: vec![CheckName::new("app:smoke").unwrap()],
        waited_s: 3_700.0,
        unrunnable: Vec::new(),
    };
    let filed = stuck::tip_waiting(&mut conn, UPKEEP, &waiting, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);

    // multivac keeps storing results while the tip still waits, which is a slow round, not a
    // fault, so the ticket closes.
    let mut closed = Vec::new();
    for n in 0..12_u32 {
        clock.advance(600.0);
        fleet::register(&conn, &machine("m-2", "multivac"), clock.now()).unwrap();
        let result = NewResult {
            key: ResultKey {
                check: CheckName::new("app:smoke").unwrap(),
                hash: InputHash::new(format!("{n:064x}")).unwrap(),
                platform: Some(Os::Macos),
            },
            outcome: Outcome::Pass,
            machine: MachineId::new("m-2").unwrap(),
            record: RecordId(1),
            output: None,
            cost: ResultCost::default(),
            areas: Vec::new(),
        };
        results::store(&mut conn, &[result], clock.now()).unwrap();
        closed.extend(upkeep.pass(&mut conn, clock.now()).unwrap().tickets_closed);
    }
    assert_eq!(closed, vec![filed[0].ticket.clone()]);
    let reason = ticket(&conn, &filed[0].ticket).close_reason.unwrap();
    assert!(reason.contains("running the tip's checks"), "{reason}");
}

#[test]
fn a_master_lag_ticket_closes_once_master_moves() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let work = apply(
        &mut conn,
        vec![create("Work", TicketType::Task, None, &[])],
        clock.now(),
    );
    conn.execute(
        "UPDATE tickets SET landed_at = ?2 WHERE id = ?1",
        params![work[0], clock.now().to_string()],
    )
    .unwrap();
    clock.advance(super::justiciar::MASTER_LAG_SECONDS + 60.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let lag = report.tickets_filed[0].clone();
    tips::seen(&conn, "abcd", clock.now()).unwrap();
    conn.execute(
        "UPDATE tips SET state = 'verified', verified = ?1",
        [clock.now().to_string()],
    )
    .unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![lag]
    );
}

fn result(conn: &mut Connection, check: &str, hash: char, outcome: Outcome, now: Timestamp) {
    let result = NewResult {
        key: ResultKey {
            check: CheckName::new(check).unwrap(),
            hash: InputHash::new(hash.to_string().repeat(64)).unwrap(),
            platform: Some(Os::Macos),
        },
        outcome,
        machine: MachineId::new("m-1").unwrap(),
        record: RecordId(1),
        output: None,
        cost: ResultCost::default(),
        areas: Vec::new(),
    };
    results::store(conn, &[result], now).unwrap();
}

#[test]
fn an_incomplete_ticket_closes_once_every_check_it_held_back_has_a_result() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let report = IncompleteReport {
        check: CheckName::new("ios:smoke").unwrap(),
        machine: MachineId::new("m-2").unwrap(),
        host: "multivac".to_owned(),
        platform: Some(Os::Macos),
        attempts: 3,
        reason: IncompleteReason::Unreported,
        commit: Commit::new("abcd").unwrap(),
        output: "ended early".to_owned(),
        checks: vec![
            CheckName::new("ios:smoke").unwrap(),
            CheckName::new("ios:launch").unwrap(),
        ],
        count: 2,
        product: None,
        holds_tip: false,
    };
    // A result stored before the ticket says nothing.
    result(&mut conn, "ios:launch", '1', Outcome::Pass, clock.now());
    clock.advance(60.0);
    let filed = stuck::incomplete(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    clock.advance(60.0);
    result(&mut conn, "ios:smoke", '2', Outcome::Fail, clock.now());
    assert!(passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).is_empty());
    result(&mut conn, "ios:launch", '3', Outcome::Pass, clock.now());
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![filed.ticket.clone()]
    );
    let reason = ticket(&conn, &filed.ticket).close_reason.unwrap();
    assert!(reason.contains("ios:smoke, ios:launch"), "{reason}");
}

fn round(commit: &str, at: Timestamp, listing_s: f64) -> TipRound {
    TipRound {
        commit: Commit::new(commit).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        at,
        wall_s: 600.0,
        ran: 10,
        cached: 2,
        checks_s: 600.0 - listing_s,
        listing_s,
        listing_alone_s: None,
        building_s: 0.0,
        waiting_s: 0.0,
        budget: None,
    }
}

#[test]
fn a_slow_round_ticket_closes_after_three_quick_rounds() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    tip_rounds::record(&conn, &round("aaaa", clock.now(), 300.0)).unwrap();
    let slow = file(
        &mut conn,
        rounds_route::SUBJECT,
        rounds_route::CATEGORY,
        clock.now(),
    );
    for commit in ["bbbb", "cccc"] {
        clock.advance(600.0);
        tip_rounds::record(&conn, &round(commit, clock.now(), 10.0)).unwrap();
    }
    assert!(passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).is_empty());
    tip_rounds::record(&conn, &round("dddd", clock.now(), 10.0)).unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![slow]
    );
}

#[test]
fn a_regression_closes_once_a_later_tip_passes_and_is_marked_fixed() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    tips::seen(&conn, "1111", clock.now()).unwrap();
    conn.execute("UPDATE tips SET state = 'failed'", [])
        .unwrap();
    clock.advance(60.0);
    let report = RegressionReport {
        check: CheckName::new("rust:a::b").unwrap(),
        first_bad: Commit::new("1111").unwrap(),
        last_good: Commit::new("0000").unwrap(),
        caused_by: None,
        output: "x".to_owned(),
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by: FoundBy::TipVerification,
    };
    let (id, _) = regressions::file(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    assert!(passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).is_empty());
    tips::seen(&conn, "2222", clock.now()).unwrap();
    conn.execute(
        "UPDATE tips SET state = 'verified', verified = seen WHERE commit_sha = '2222'",
        [],
    )
    .unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![id.clone()]
    );
    let state: String = conn
        .query_row("SELECT state FROM regressions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, tips::FIXED);
    assert!(
        ticket(&conn, &id)
            .close_reason
            .unwrap()
            .contains("tip 2222")
    );

    // The check fails again: the ticket reopens, and says why.
    let (again, _) = regressions::file(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    assert_eq!(again, id);
    let comments = tickets::comments(&conn, &id).unwrap();
    assert!(comments.last().unwrap().text.contains("fails again"));
}

#[test]
fn a_ticket_given_another_check_waits_for_a_tip_seen_after_it() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let report = |check: &str, first_bad: &str| RegressionReport {
        check: CheckName::new(check).unwrap(),
        first_bad: Commit::new(first_bad).unwrap(),
        last_good: Commit::new("0000").unwrap(),
        caused_by: None,
        output: "x".to_owned(),
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by: FoundBy::TipVerification,
    };
    let (id, _) = regressions::file(
        &mut conn,
        UPKEEP,
        &report("rust:a::one", "1111"),
        clock.now(),
    )
    .unwrap();
    clock.advance(60.0);
    tips::seen(&conn, "4444", clock.now()).unwrap();
    conn.execute(
        "UPDATE tips SET state = 'verified', verified = seen WHERE commit_sha = '4444'",
        [],
    )
    .unwrap();
    // A check from the first ticket's landing fails after tip 4444 was seen, and another
    // machine's report adds it to the ticket.
    clock.advance(60.0);
    let other = filing::Author {
        actor: "daemon-m-2",
        machine: "m-2",
    };
    let (joined, outcome) = regressions::file(
        &mut conn,
        other,
        &report("rust:a::four", "1111"),
        clock.now(),
    )
    .unwrap();
    assert_eq!(
        (joined.as_str(), outcome),
        (id.as_str(), regressions::Outcome::Added)
    );
    assert!(!passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).contains(&id));
    tips::seen(&conn, "5555", clock.now()).unwrap();
    conn.execute(
        "UPDATE tips SET state = 'verified', verified = seen WHERE commit_sha = '5555'",
        [],
    )
    .unwrap();
    assert!(passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS).contains(&id));
    let fixed: i64 = conn
        .query_row(
            "SELECT count(*) FROM regressions WHERE ticket = ?1 AND state = ?2",
            params![id, tips::FIXED],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fixed, 2);
}

#[test]
fn a_loop_ticket_closes_once_an_edge_of_the_loop_is_removed() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let a = apply(
        &mut conn,
        vec![create("A", TicketType::Task, None, &[])],
        clock.now(),
    );
    let b = apply(
        &mut conn,
        vec![create("B", TicketType::Task, None, &[&a[0]])],
        clock.now(),
    );
    // The store refuses an edge that closes a loop, so the loop is written to the table.
    edges::add(&conn, &a[0], &b[0], EdgeKind::Blocks).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let repair = report.tickets_filed[0].clone();
    edges::remove(&conn, &a[0], &b[0]).unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        vec![repair.clone()]
    );
    let reason = ticket(&conn, &repair).close_reason.unwrap();
    assert!(reason.contains("no longer wait on one another"), "{reason}");
}

#[test]
fn turning_off_a_machine_closes_its_waiting_ticket_while_the_tip_still_waits() {
    use ignite_core::fleet::MachineState;
    use ignite_store::hub::fleet_settings;

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    fleet::register(&conn, &machine("m-2", "multivac"), clock.now()).unwrap();
    clock.advance(60.0);
    tips::seen(&conn, "abcd", clock.now()).unwrap();
    clock.advance(3_700.0);
    let waiting = upkeep.pass(&mut conn, clock.now()).unwrap().tickets_filed;
    assert_eq!(waiting.len(), 1);

    fleet_settings::set_machine_state(&mut conn, "m-2", MachineState::Off, "operator", clock.now())
        .unwrap();
    assert_eq!(
        passes(&mut conn, &mut upkeep, &clock, 2.0 * HOLD_SECONDS),
        waiting
    );
    let reason = ticket(&conn, &waiting[0]).close_reason.unwrap();
    assert!(reason.contains("turned off"), "{reason}");
}
