//! Justiciar tickets from reports and from a `master` that stopped moving.

use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId};
use ignite_core::protocol::hub::{
    JusticiarReport, ResultKey, TipCheckFailures, TipFailure, TipState, TipVerified,
};
use ignite_core::protocol::tickets::{CULPRIT_HANDOFF_KEY, Operation, Status, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};
use ignite_core::traits::Clock;
use ignite_store::hub::{self, tips};
use rusqlite::{Connection, params};
use serde_json::json;

use super::filing::{Change, UPKEEP};
use super::justiciar::{self, MASTER_LAG_SECONDS};
use super::regression_stalls::{FALLBACK_AGE_SECONDS, INACTIVE_SECONDS, UNCLAIMED_SECONDS};
use super::tests::{all_tickets, apply, comment_count, create, start, ticket};

fn crash(machine: &str, host: &str, evidence: &str, on_machine: bool) -> JusticiarReport {
    JusticiarReport {
        subject: "crash:ignite-tip".to_owned(),
        title: "The tip loop panicked".to_owned(),
        details: "index out of bounds at tip.rs:12".to_owned(),
        evidence: evidence.to_owned(),
        machine: MachineId::new(machine).unwrap(),
        host: host.to_owned(),
        on_machine,
    }
}

#[test]
fn a_repeated_report_comments_on_one_p0_ticket_offered_first() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let report = crash("m-1", "jarvis", "crash-1", false);
    let first = justiciar::report(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    assert_eq!(first.change, Change::Created);
    assert!(first.ticket.starts_with('J'), "{}", first.ticket);
    let filed = ticket(&conn, &first.ticket);
    assert_eq!(filed.priority, 0);
    // An emergency cannot wait for a person to release its ticket.
    assert_eq!(filed.status, Status::Open);
    for label in ["justiciar", "tier:opus"] {
        assert!(
            filed.labels.iter().any(|l| l == label),
            "{:?}",
            filed.labels
        );
    }
    assert!(filed.body.contains("ignite guide justiciar"));

    let again = justiciar::report(&mut conn, UPKEEP, &report, clock.now()).unwrap();
    assert_eq!(again.change, Change::Unchanged);
    let later = crash("m-1", "jarvis", "crash-2", false);
    let second = justiciar::report(&mut conn, UPKEEP, &later, clock.now()).unwrap();
    assert_eq!(second.ticket, first.ticket);
    assert_eq!(second.change, Change::Commented);
    assert_eq!(comment_count(&conn, &first.ticket), 1);
}

#[test]
fn a_machine_only_report_needs_that_machine_and_has_a_ticket_per_machine() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let jarvis = crash("m-1", "jarvis", "crash-1", true);
    let cygnus = crash("m-2", "cygnus", "crash-1", true);
    let a = justiciar::report(&mut conn, UPKEEP, &jarvis, clock.now()).unwrap();
    let b = justiciar::report(&mut conn, UPKEEP, &cygnus, clock.now()).unwrap();
    assert_ne!(a.ticket, b.ticket);
    assert!(
        ticket(&conn, &a.ticket)
            .labels
            .contains(&"machine:jarvis".to_owned())
    );
    assert!(
        ticket(&conn, &b.ticket)
            .labels
            .contains(&"machine:cygnus".to_owned())
    );
    // It holds work back, so it may fall back when its machine goes silent.
    assert!(ticket(&conn, &a.ticket).fallback);
}

fn land(conn: &mut rusqlite::Connection, id: &str, at: &str) {
    conn.execute("UPDATE tickets SET landed_at = ?1 WHERE id = ?2", [at, id])
        .unwrap();
}

fn verify(conn: &rusqlite::Connection, commit: &str, at: &str) {
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, verified, state) VALUES (?1, ?2, ?2, 'verified')",
        [commit, at],
    )
    .unwrap();
}

fn lag_tickets(conn: &rusqlite::Connection, clock: &ignite_core::fakes::FakeClock) -> usize {
    all_tickets(conn, clock.now())
        .iter()
        .filter(|t| t.title.starts_with("master has not moved"))
        .count()
}

#[test]
fn a_landing_that_master_lacks_for_two_hours_files_one_open_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let landed = apply(
        &mut conn,
        vec![create("work", TicketType::Task, None, &[])],
        clock.now(),
    );
    land(&mut conn, &landed[0], &clock.now().to_string());

    clock.advance(MASTER_LAG_SECONDS - 60.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );

    clock.advance(120.0);
    let filed = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    let lag = ticket(&conn, &filed[0].ticket);
    assert!(lag.id.as_str().starts_with('J'), "{}", lag.id);
    assert!(lag.body.contains(&landed[0]));
    assert_eq!(lag.title, "master has not moved for 121 minutes");
    assert_eq!(lag.status, Status::Open);
    assert!((MASTER_LAG_SECONDS - 2.0 * 3600.0).abs() < f64::EPSILON);
    // Another pass sees the same stuck landing and adds nothing.
    let again = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(again[0].change, Change::Unchanged);
    assert_eq!(lag_tickets(&conn, &clock), 1);
}

#[test]
fn master_lag_reports_candidate_failures_before_completion_and_keeps_their_commit() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let _ = landing(&mut conn, &clock);
    let candidate = |commit| TipCandidate {
        commit: Commit::new(commit).unwrap(),
        retry_on_new_tip: false,
    };
    let tip = candidate("bbbb000");
    assert!(
        tips::update_candidate(
            &mut conn,
            &TipCandidateUpdate {
                expected: None,
                replacement: tip.clone(),
            },
            clock.now()
        )
        .unwrap()
    );
    clock.advance(MASTER_LAG_SECONDS);
    let lag = justiciar::master_lag(&mut conn, clock.now())
        .unwrap()
        .remove(0);
    assert_eq!(lag.change, Change::Created);

    let report = TipCheckFailures {
        commit: tip.commit.clone(),
        checks: vec![ResultKey {
            check: CheckName::new("unit:broken").unwrap(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform: None,
        }],
    };
    tips::note_check_failures(&mut conn, &report).unwrap();
    let updated = justiciar::master_lag(&mut conn, clock.now())
        .unwrap()
        .remove(0);
    assert_eq!(updated.ticket, lag.ticket);
    assert_eq!(updated.change, Change::Commented);
    let comments = |conn: &Connection| {
        let mut query = conn
            .prepare("SELECT text FROM comments WHERE ticket = ?1 ORDER BY n")
            .unwrap();
        query
            .query_map([&lag.ticket], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert!(
        comments(&conn)[0].contains("Required checks reported failures at candidate `bbbb000`")
    );
    assert!(comments(&conn)[0].contains("`unit:broken`"));
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].change,
        Change::Unchanged
    );

    finish_tip(&conn, &clock, "bbbb000", "unit:broken", &[]);
    assert!(
        tips::update_candidate(
            &mut conn,
            &TipCandidateUpdate {
                expected: Some(TipCandidate {
                    retry_on_new_tip: true,
                    ..tip
                }),
                replacement: candidate("cccc000"),
            },
            clock.now()
        )
        .unwrap()
    );
    tips::note_check_failures(&mut conn, &report).unwrap();
    let previous = justiciar::master_lag(&mut conn, clock.now())
        .unwrap()
        .remove(0);
    assert_eq!(previous.ticket, lag.ticket);
    assert_eq!(previous.change, Change::Commented);
    let notes = comments(&conn);
    assert!(notes[1].contains("A completed round recorded failures at `bbbb000`"));
    assert!(notes[1].contains("Any later candidate needs results for its own check inputs."));
}

#[test]
fn a_tip_verified_after_the_landing_means_no_lag() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let landed = apply(
        &mut conn,
        vec![create("work", TicketType::Task, None, &[])],
        clock.now(),
    );
    land(&mut conn, &landed[0], &clock.now().to_string());
    clock.advance(600.0);
    verify(&conn, "abc1234", &clock.now().to_string());
    clock.advance(MASTER_LAG_SECONDS * 2.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn landings_older_than_a_week_never_count_as_lag() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let landed = apply(
        &mut conn,
        vec![create("imported", TicketType::Task, None, &[])],
        clock.now(),
    );
    land(&mut conn, &landed[0], &clock.now().to_string());
    clock.advance(8.0 * 86_400.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );
}

/// Records a landing of a new ticket at `clock`'s time and returns the ticket.
fn landing(conn: &mut rusqlite::Connection, clock: &ignite_core::fakes::FakeClock) -> String {
    let landed = apply(
        conn,
        vec![create("work", TicketType::Task, None, &[])],
        clock.now(),
    );
    land(conn, &landed[0], &clock.now().to_string());
    landed[0].clone()
}

fn close(conn: &mut rusqlite::Connection, id: &str, at: &str) {
    conn.execute(
        "UPDATE tickets SET status = 'closed', closed_at = ?1 WHERE id = ?2",
        [at, id],
    )
    .unwrap();
}

#[test]
fn an_open_regression_ticket_is_told_once_per_stuck_landing_instead_of_a_new_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 60.0);
    let regression = regression(&mut conn, &clock);
    let told = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].ticket, regression);
    assert_eq!(told[0].change, Change::Commented);
    assert_eq!(lag_tickets(&conn, &clock), 0);
    let held = ticket(&conn, &regression);
    // The daemon holding the ticket delivers the comment with this number to its session.
    assert_eq!(held.metadata.get("tell"), Some(&serde_json::json!(1)));
    assert_eq!(comment_count(&conn, &regression), 1);

    // Later passes about the same stuck landing tell it nothing more.
    clock.advance(UNCLAIMED_SECONDS - 1.0);
    let again = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(again[0].change, Change::Unchanged);
    assert_eq!(comment_count(&conn, &regression), 1);

    // Once the regression ticket is closed, a lag that remains is no longer explained.
    close(&mut conn, &regression, &clock.now().to_string());
    let filed = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(filed[0].change, Change::Created);
    assert_eq!(lag_tickets(&conn, &clock), 1);
}

#[test]
fn a_closed_ticket_for_the_same_stuck_landing_waits_before_a_new_one() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 60.0);
    let first = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    close(&mut conn, &first[0].ticket, &clock.now().to_string());

    // A worker closed it, but `master` still lacks the same landing.
    clock.advance(MASTER_LAG_SECONDS - 60.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );
    assert_eq!(lag_tickets(&conn, &clock), 1);

    // The stall outlasted the wait, so the fix did not work and a new ticket is filed.
    clock.advance(120.0);
    let second = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(second[0].change, Change::Created);
    assert_ne!(second[0].ticket, first[0].ticket);
}

#[test]
fn a_closed_ticket_for_an_earlier_landing_does_not_hold_back_a_new_stall() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 60.0);
    let first = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    close(&mut conn, &first[0].ticket, &clock.now().to_string());

    // `master` moves past the first landing, and a later one gets stuck.
    clock.advance(60.0);
    verify(&conn, "abc1234", &clock.now().to_string());
    clock.advance(60.0);
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 60.0);
    let second = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(second.len(), 1);
    assert_ne!(second[0].ticket, first[0].ticket);
}

fn regression(conn: &mut Connection, clock: &FakeClock) -> String {
    let ids = apply(
        conn,
        vec![create("pytest fails", TicketType::Bug, None, &[])],
        clock.now(),
    );
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('quality:unit', ?1, 'bbbbbbb', 'aaaaaaa', 'open')",
        [&ids[0]],
    )
    .unwrap();
    ids[0].clone()
}

fn claim(conn: &mut Connection, ticket: &str, now: Timestamp) -> String {
    apply(
        conn,
        vec![Operation::Claim {
            id: ticket.to_owned(),
        }],
        now,
    );
    let actor: String = conn
        .query_row(
            "SELECT assignee FROM tickets WHERE id = ?1",
            [ticket],
            |row| row.get(0),
        )
        .unwrap();
    actor
}

fn handoff(conn: &mut Connection, ticket: &str, result: serde_json::Value, now: Timestamp) {
    apply(
        conn,
        vec![Operation::Update {
            id: ticket.to_owned(),
            set: None,
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: [(CULPRIT_HANDOFF_KEY.to_owned(), result)].into(),
            unset_meta: Vec::new(),
        }],
        now,
    );
}

fn record(
    conn: &Connection,
    ticket: &str,
    holder: &str,
    now: Timestamp,
    verb: &str,
    outcome: &str,
) {
    conn.execute(
        "INSERT INTO records (ticket, session, at, verb, machine, class, queued_s, wall_s, cpu_s,
                              peak_mb, outcome, ignite)
         VALUES (?1, ?2, ?3, ?4, 'machine', 'agent', 0, 0, 0, 0, ?5, 'test')",
        params![ticket, holder, now.to_string(), verb, outcome],
    )
    .unwrap();
}

fn escalate(conn: &mut Connection, clock: &FakeClock, regression: &str, reason: &str) -> String {
    let filed = justiciar::master_lag(conn, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    let emergency = ticket(conn, &filed[0].ticket);
    assert_eq!(emergency.status, Status::Open);
    assert_eq!(emergency.priority, 0);
    assert!(
        emergency
            .labels
            .iter()
            .any(|label| label == justiciar::LABEL)
    );
    assert!(emergency.body.contains(regression));
    assert!(emergency.body.contains(reason), "{}", emergency.body);
    emergency.id.to_string()
}

#[test]
fn a_regression_waiting_for_a_claim_escalates_and_clears_when_work_starts() {
    use super::clearing::{HOLD_SECONDS, close_cleared};

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 1.0);
    let regression = regression(&mut conn, &clock);
    clock.advance(UNCLAIMED_SECONDS - 1.0);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );

    clock.advance(1.0);
    let emergency = escalate(&mut conn, &clock, &regression, "without a claim");
    assert!(
        justiciar::yields_to_repair(&conn, &emergency, clock.now(), |id| id == regression).unwrap()
    );
    assert!(!justiciar::yields_to_repair(&conn, &emergency, clock.now(), |_| false).unwrap());
    let again = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(again[0].ticket, emergency);
    assert_eq!(again[0].change, Change::Unchanged);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());

    let holder = claim(&mut conn, &regression, clock.now());
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(HOLD_SECONDS);
    record(&conn, &regression, &holder, clock.now(), "check", "pass");
    assert_eq!(close_cleared(&mut conn, clock.now()).unwrap(), [emergency]);
}

#[test]
fn a_regression_whose_holder_stops_recording_activity_escalates() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let regression = regression(&mut conn, &clock);
    let holder = claim(&mut conn, &regression, clock.now());
    clock.advance(MASTER_LAG_SECONDS + 1.0);
    record(&conn, &regression, &holder, clock.now(), "test", "pass");

    clock.advance(INACTIVE_SECONDS - 1.0);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );
    clock.advance(1.0);
    let emergency = escalate(
        &mut conn,
        &clock,
        &regression,
        "holder last recorded activity",
    );
    assert!(
        !justiciar::yields_to_repair(&conn, &emergency, clock.now(), |id| id == regression)
            .unwrap()
    );

    clock.advance(1.0);
    apply(
        &mut conn,
        vec![Operation::Comment {
            id: regression.clone(),
            text: "Working on this".to_owned(),
        }],
        clock.now(),
    );
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );
}

/// A CLI worker's `ignite test` runs reach the hub without a ticket, and its `ignite run`
/// commands show only on its worker row. Either keeps its regression from looking stalled.
#[test]
fn a_holder_that_only_tests_and_runs_commands_is_active() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let regression = regression(&mut conn, &clock);
    let holder = claim(&mut conn, &regression, clock.now());
    clock.advance(MASTER_LAG_SECONDS + 1.0);
    conn.execute(
        "INSERT INTO records (ticket, session, at, verb, machine, class, queued_s, wall_s, cpu_s,
                              peak_mb, outcome, ignite)
         VALUES (NULL, ?1, ?2, 'test', 'machine', 'agent', 0, 0, 0, 0, 'fail', 'test')",
        params![holder, clock.now().to_string()],
    )
    .unwrap();
    clock.advance(INACTIVE_SECONDS - 1.0);
    // The claim is older than the threshold, so only the record without a ticket holds it off.
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );
    conn.execute(
        "INSERT INTO workers (machine, session, app, role, state, ticket, command, command_since,
                              command_running)
         VALUES ('m-1', ?1, 'claude', 'worker', 'working', ?2, 'run --', ?3, 1)",
        params![holder, regression, clock.now().to_string()],
    )
    .unwrap();

    clock.advance(INACTIVE_SECONDS - 1.0);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );
    clock.advance(1.0);
    escalate(
        &mut conn,
        &clock,
        &regression,
        "holder last recorded activity",
    );
}

fn finish_tip(conn: &Connection, clock: &FakeClock, commit: &str, check: &str, contains: &[&str]) {
    tips::verified(
        conn,
        &TipVerified {
            commit: Commit::new(commit).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Failed,
            regression_passes: None,
            failed_descendant: None,
            failure: Some(TipFailure {
                checks: vec![ResultKey {
                    check: CheckName::new(check).unwrap(),
                    hash: InputHash::new("a".repeat(64)).unwrap(),
                    platform: None,
                }],
                unverified_commits: contains
                    .iter()
                    .map(|sha| Commit::new(*sha).unwrap())
                    .collect(),
            }),
        },
        clock.now(),
    )
    .unwrap();
}

fn fix_attempt(conn: &Connection, regression: &str, clock: &FakeClock, sha: &str) {
    crate::routes::dispatch::set_columns(
        conn,
        regression,
        "worker",
        &[(
            "landed",
            json!({"commits": [sha], "files": ["tests.rs"], "note": "fix attempt"}),
        )],
        clock.now(),
    )
    .unwrap();
}

#[test]
fn only_separate_landed_fixes_verified_to_fail_the_same_check_escalate() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 1.0);
    let regression = regression(&mut conn, &clock);
    claim(&mut conn, &regression, clock.now());

    clock.advance(1.0);
    fix_attempt(&conn, &regression, &clock, "cafe");
    finish_tip(&conn, &clock, "cafe", "quality:unit", &[]);
    finish_tip(&conn, &clock, "cafe1", "quality:unit", &["cafe"]);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );

    clock.advance(1.0);
    fix_attempt(&conn, &regression, &clock, "babe");
    finish_tip(&conn, &clock, "babe", "quality:other", &[]);
    finish_tip(&conn, &clock, "ca1f", "quality:unit", &[]);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );

    finish_tip(&conn, &clock, "face", "quality:unit", &["cafe", "babe"]);
    escalate(&mut conn, &clock, &regression, "2 separate fixes landed");
}

#[test]
fn a_regression_outlying_the_historical_lifetime_escalates_even_with_recent_activity() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let regression = regression(&mut conn, &clock);
    clock.advance(FALLBACK_AGE_SECONDS - 1.0);
    claim(&mut conn, &regression, clock.now());
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );

    clock.advance(1.0);
    escalate(&mut conn, &clock, &regression, "historical 90th-percentile");
}

#[test]
fn a_holder_who_resumed_the_culprit_gets_time_until_it_falls_back() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let regression = regression(&mut conn, &clock);
    let holder = claim(&mut conn, &regression, clock.now());
    handoff(
        &mut conn,
        &regression,
        json!({"session": holder, "note": "Resumed the culprit's transcript from an earlier ticket"}),
        clock.now(),
    );

    clock.advance(FALLBACK_AGE_SECONDS + 1.0);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );

    handoff(
        &mut conn,
        &regression,
        json!({"session": holder, "resumed": false, "note": "Started fresh after a failed resume"}),
        clock.now(),
    );
    escalate(&mut conn, &clock, &regression, "historical 90th-percentile");
}

#[test]
fn a_regression_escalation_clears_when_a_tip_passes() {
    use super::clearing::{HOLD_SECONDS, close_cleared};

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let regression = regression(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS + 1.0);
    let emergency = escalate(&mut conn, &clock, &regression, "without a claim");

    verify(&conn, "abcd", &clock.now().to_string());
    assert!(
        !close_cleared(&mut conn, clock.now())
            .unwrap()
            .contains(&emergency)
    );
    clock.advance(HOLD_SECONDS);
    assert!(
        close_cleared(&mut conn, clock.now())
            .unwrap()
            .contains(&emergency)
    );
}

#[test]
fn recent_culprit_resumes_set_the_age_threshold_when_there_are_enough() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    for lifetime in 1..=20 {
        let sample = apply(
            &mut conn,
            vec![create("earlier regression", TicketType::Bug, None, &[])],
            clock.now(),
        );
        let holder = "earlier-worker";
        handoff(
            &mut conn,
            &sample[0],
            json!({"session": holder, "resumed": true}),
            clock.now(),
        );
        conn.execute(
            "UPDATE tickets SET status = 'closed', labels = '[\"regression\"]',
                                closed_at = ?1, session = ?2 WHERE id = ?3",
            params![
                clock
                    .now()
                    .plus_seconds(60.0 * f64::from(lifetime))
                    .to_string(),
                holder,
                sample[0]
            ],
        )
        .unwrap();
    }
    clock.advance(MASTER_LAG_SECONDS + 1.0);
    let regression = regression(&mut conn, &clock);
    claim(&mut conn, &regression, clock.now());

    clock.advance(18.0 * 60.0 - 1.0);
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap()[0].ticket,
        regression
    );
    clock.advance(1.0);
    escalate(&mut conn, &clock, &regression, "historical 90th-percentile");
}

#[test]
fn a_verified_tip_that_did_not_run_the_regression_check_leaves_its_stall_open() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let regression = regression(&mut conn, &clock);
    clock.advance(1.0);
    tips::verified(
        &conn,
        &TipVerified {
            commit: Commit::new("caf1").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Verified,
            regression_passes: Some(Vec::new()),
            failed_descendant: None,
            failure: None,
        },
        clock.now(),
    )
    .unwrap();
    clock.advance(1.0);
    landing(&mut conn, &clock);
    clock.advance(MASTER_LAG_SECONDS);
    escalate(&mut conn, &clock, &regression, "without a claim");
}
