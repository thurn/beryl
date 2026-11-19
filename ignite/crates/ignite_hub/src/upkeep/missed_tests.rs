//! Recording why tip verification missed a check that a landing found failing.

use rusqlite::{Connection, params};

use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, MachineId};
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::time::Timestamp;
use ignite_core::tipround::{RoundBudget, TipRound};
use ignite_core::traits::Clock;
use ignite_store::hub::{self, tickets, tip_misses, tip_rounds, tips};

use super::filing::UPKEEP;
use super::missed::{self, CATEGORY, Kind};
use super::regressions;
use super::tests::{all_tickets, start};

fn report(check: &str, tip: &str, found_by: FoundBy) -> RegressionReport {
    RegressionReport {
        check: CheckName::new(check).unwrap(),
        first_bad: Commit::new(tip).unwrap(),
        last_good: Commit::new("0000").unwrap(),
        caused_by: None,
        output: "assertion failed".to_owned(),
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by,
    }
}

/// A finished round at `tip` that left `unrun` checks of the `unit` family unrun, ended by
/// `ended_by` when one is named.
fn round(conn: &Connection, clock: &FakeClock, tip: &str, unrun: u32, ended_by: Option<&str>) {
    tips::seen(conn, tip, clock.now()).unwrap();
    let mut budget = RoundBudget::default();
    if unrun > 0 {
        budget.unrun.insert("unit".to_owned(), unrun);
    }
    budget.ended_by = ended_by.map(|c| Commit::new(c).unwrap());
    let round = TipRound {
        commit: Commit::new(tip).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        at: clock.now(),
        wall_s: 60.0,
        ran: 1,
        cached: 0,
        checks_s: 50.0,
        listing_s: 1.0,
        building_s: 0.0,
        waiting_s: 0.0,
        budget: Some(budget),
    };
    tip_rounds::record(conn, &round).unwrap();
}

/// Files `report` a minute later and returns its regression ticket and the miss recorded.
fn file(
    conn: &mut Connection,
    clock: &FakeClock,
    report: &RegressionReport,
) -> (String, Option<(String, String)>) {
    clock.advance(60.0);
    let (ticket, _) = regressions::file(conn, UPKEEP, report, clock.now()).unwrap();
    let miss = tip_misses::since(conn, None, Timestamp::EPOCH)
        .unwrap()
        .into_iter()
        .find(|m| m.check_name == report.check.as_str() && m.tip == report.first_bad.as_str())
        .map(|m| (m.kind, m.reason));
    (ticket, miss)
}

fn texts(conn: &Connection, ticket: &str) -> Vec<String> {
    tickets::comments(conn, ticket)
        .unwrap()
        .into_iter()
        .map(|c| c.text)
        .collect()
}

#[test]
fn nothing_is_recorded_before_tip_verification_reaches_the_tip() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    // The tip is being verified, but no round has finished there.
    tips::seen(&conn, "abcd", clock.now()).unwrap();
    let (ticket, miss) = file(
        &mut conn,
        &clock,
        &report("unit:a", "abcd", FoundBy::Landing),
    );
    assert_eq!(miss, None);
    assert!(texts(&conn, &ticket).is_empty());
}

#[test]
fn a_report_from_tip_verification_is_no_miss() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    round(&conn, &clock, "abcd", 0, None);
    let report = report("unit:a", "abcd", FoundBy::TipVerification);
    assert_eq!(file(&mut conn, &clock, &report).1, None);
}

/// A round finished at the tip, ran everything it set out to, and never had the check: its
/// declared inputs did not change, so the last pass stood.
#[test]
fn a_finished_round_that_left_nothing_unrun_did_not_have_the_check() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    round(&conn, &clock, "abcd1234ef", 0, None);
    let (ticket, miss) = file(
        &mut conn,
        &clock,
        &report("unit:a", "abcd1234ef", FoundBy::Landing),
    );
    let (kind, reason) = miss.unwrap();
    assert_eq!(kind, Kind::NotRun.as_str());
    assert!(reason.contains("at that tip (abcd1234e)"), "{reason}");
    let comments = texts(&conn, &ticket);
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(
        comments[0].starts_with("Tip verification missed unit:a: a landing found it failing"),
        "{}",
        comments[0]
    );
    assert!(comments[0].contains(&reason));
}

/// The incident this records: the round at the tip moved `master`, and a newer tip ended it
/// before it ran the checks past the budget, the failing one among them.
#[test]
fn a_round_a_newer_tip_ended_skipped_the_check() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    round(&conn, &clock, "abcd", 36, Some("ef01"));
    let (_, miss) = file(
        &mut conn,
        &clock,
        &report("unit:a", "abcd", FoundBy::Landing),
    );
    let (kind, reason) = miss.unwrap();
    assert_eq!(kind, Kind::Skipped.as_str());
    assert!(reason.contains("left 36 unit checks unrun"), "{reason}");
    assert!(
        reason.contains("the newer tip ef01 ended the round"),
        "{reason}"
    );

    // When `master` did not move, the checks past the budget wait for a later round.
    round(&conn, &clock, "2222", 4, None);
    let (_, miss) = file(
        &mut conn,
        &clock,
        &report("unit:b", "2222", FoundBy::Landing),
    );
    let (kind, reason) = miss.unwrap();
    assert_eq!(kind, Kind::Skipped.as_str());
    assert!(reason.contains("`master` did not move"), "{reason}");
}

/// A later round that ran every check of the family outranks an earlier one that skipped it:
/// that round should have run the check.
#[test]
fn a_later_round_that_left_nothing_unrun_outranks_a_skip() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    round(&conn, &clock, "abcd", 3, Some("ef01"));
    clock.advance(60.0);
    round(&conn, &clock, "ef01", 0, None);
    let (_, miss) = file(
        &mut conn,
        &clock,
        &report("unit:a", "abcd", FoundBy::Landing),
    );
    let (kind, reason) = miss.unwrap();
    assert_eq!(kind, Kind::NotRun.as_str());
    assert!(reason.contains("at the later tip ef01"), "{reason}");
}

/// Tip verification passed the check at the tip: a pass the landing's failure shows is stale.
#[test]
fn a_tip_round_pass_at_the_tip_is_a_stale_pass() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    round(&conn, &clock, "abcd", 0, None);
    conn.execute(
        "INSERT INTO records (id, machine, verb, class, family, queued_s, wall_s, cpu_s, peak_mb,
                              commit_sha, ignite, at)
         VALUES (7, 'm-1', 'check', 'tip', 'unit', 0, 1, 1, 1, 'abcd', 'x', ?1)",
        params![clock.now().to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO results (check_name, hash, platform, outcome, machine, record, at,
                              last_lookup)
         VALUES ('unit:a', 'h1', '', 'pass', 'm-1', 7, ?1, ?1)",
        params![clock.now().to_string()],
    )
    .unwrap();
    let (_, miss) = file(
        &mut conn,
        &clock,
        &report("unit:a", "abcd", FoundBy::Landing),
    );
    assert_eq!(miss.unwrap().0, Kind::StalePass.as_str());
}

fn family_tickets(conn: &Connection, clock: &FakeClock) -> Vec<String> {
    let label = format!("rule:{CATEGORY}");
    all_tickets(conn, clock.now())
        .into_iter()
        .filter(|t| t.labels.contains(&label))
        .map(|t| t.id.to_string())
        .collect()
}

/// Three misses of one family within seven days file one `hub` ticket naming them; a fourth
/// adds a comment to it. A miss older than the window does not count.
#[test]
fn three_misses_of_a_family_in_a_week_file_one_ticket() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    round(&conn, &clock, "0001", 0, None);
    file(
        &mut conn,
        &clock,
        &report("unit:old", "0001", FoundBy::Landing),
    );
    clock.advance(missed::WINDOW_SECONDS);
    for (check, tip) in [("unit:a", "0002"), ("unit:b", "0003")] {
        round(&conn, &clock, tip, 0, None);
        file(&mut conn, &clock, &report(check, tip, FoundBy::Landing));
    }
    // A miss of another family counts for its own family only.
    round(&conn, &clock, "0004", 0, None);
    file(
        &mut conn,
        &clock,
        &report("rust:x", "0004", FoundBy::Landing),
    );
    assert!(family_tickets(&conn, &clock).is_empty());

    round(&conn, &clock, "0005", 0, None);
    file(
        &mut conn,
        &clock,
        &report("unit:c", "0005", FoundBy::Landing),
    );
    let filed = family_tickets(&conn, &clock);
    assert_eq!(filed.len(), 1);
    let ticket = tickets::load(&conn, &filed[0]).unwrap().unwrap();
    assert!(
        ticket.labels.iter().any(|l| l == "hub"),
        "{:?}",
        ticket.labels
    );
    assert_eq!(ticket.title, "Tip verification misses failing unit checks");
    for check in ["unit:a", "unit:b", "unit:c"] {
        assert!(ticket.body.contains(check), "{}", ticket.body);
    }
    assert!(!ticket.body.contains("unit:old"));
    assert!(ticket.body.contains("not-run"));

    round(&conn, &clock, "0006", 0, None);
    file(
        &mut conn,
        &clock,
        &report("unit:d", "0006", FoundBy::Landing),
    );
    assert_eq!(family_tickets(&conn, &clock), filed);
    assert_eq!(texts(&conn, &filed[0]).len(), 1);
}
