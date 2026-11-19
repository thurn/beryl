//! One ticket per cause of incomplete checks, from new reports and from an older daemon's
//! report per check.

use std::collections::BTreeSet;

use rusqlite::Connection;

use ignite_core::ids::{CheckName, Commit, MachineId};
use ignite_core::protocol::hub::{IncompleteReason, IncompleteReport};
use ignite_core::protocol::tickets::Status;
use ignite_core::traits::Clock;
use ignite_store::hub;

use super::super::super::filing::{Change, UPKEEP};
use super::super::super::tests::{all_tickets, comment_count, start, ticket};
use super::{ROUND_LIMIT, incomplete};

fn report(check: &str, reason: IncompleteReason, output: &str) -> IncompleteReport {
    IncompleteReport {
        check: CheckName::new(check).unwrap(),
        machine: MachineId::new("m-cf83bfb6").unwrap(),
        host: "jarvis".to_owned(),
        platform: None,
        attempts: 1,
        reason,
        commit: Commit::new("7322d1425").unwrap(),
        output: output.to_owned(),
        checks: Vec::new(),
        count: 0,
        product: None,
        holds_tip: false,
    }
}

fn open_titles(conn: &Connection) -> Vec<String> {
    all_tickets(conn, start().now())
        .into_iter()
        .map(|t| t.title)
        .collect()
}

/// One refused build of `rust-tests` held back 1,446 Rust tests, and a daemon that sent one
/// report per check filed 1,446 P1 tickets. The same reports now file one.
#[test]
fn an_older_daemons_report_per_check_of_one_failed_build_files_one_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut tickets = BTreeSet::new();
    for i in 0..1_400 {
        let one = report(
            &format!("rust:rules::t{i}"),
            IncompleteReason::Unreported,
            "a build it needs failed",
        );
        tickets.insert(
            incomplete(&mut conn, UPKEEP, &one, clock.now())
                .unwrap()
                .ticket,
        );
    }
    assert_eq!(tickets.len(), 1, "{tickets:?}");
    let filed = ticket(&conn, tickets.first().unwrap());
    assert!(filed.title.starts_with("Build fails: "), "{}", filed.title);
    assert!(
        filed.body.starts_with("The build of "),
        "the first sentence states the cause: {}",
        filed.body
    );
    assert!(
        !filed.body.contains("ended without reporting"),
        "{}",
        filed.body
    );
}

#[test]
fn a_failed_build_holding_back_1400_checks_files_one_ticket_and_a_second_sighting_adds_to_it() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut build = report(
        "rust:build",
        IncompleteReason::BuildFailed,
        "error: building rust-tests for link_iroh enables different features",
    );
    build.product = Some("rust-tests".to_owned());
    build.count = 1_400;
    build.checks = (0..20)
        .map(|i| CheckName::new(format!("rust:t{i}")).unwrap())
        .collect();
    let first = incomplete(&mut conn, UPKEEP, &build, clock.now()).unwrap();
    assert_eq!(first.change, Change::Created);
    let filed = ticket(&conn, &first.ticket);
    assert_eq!(filed.title, "Build fails: rust-tests");
    assert_eq!(filed.priority, 1);
    assert!(filed.body.contains("1400 checks"), "{}", filed.body);
    assert!(
        filed.body.contains("- rust:t19\n- and 1380 more"),
        "{}",
        filed.body
    );
    assert!(
        filed.body.contains("enables different features"),
        "{}",
        filed.body
    );

    // The same report again changes nothing; the same build at a later commit, or holding back
    // another family's checks, adds a comment to the same ticket.
    assert_eq!(
        incomplete(&mut conn, UPKEEP, &build, clock.now())
            .unwrap()
            .change,
        Change::Unchanged
    );
    build.commit = Commit::new("1963793e7").unwrap();
    let second = incomplete(&mut conn, UPKEEP, &build, clock.now()).unwrap();
    assert_eq!(
        (second.ticket.as_str(), second.change),
        (first.ticket.as_str(), Change::Commented)
    );
    build.check = CheckName::new("iroh:build").unwrap();
    build.machine = MachineId::new("m-2").unwrap();
    let third = incomplete(&mut conn, UPKEEP, &build, clock.now()).unwrap();
    assert_eq!(third.ticket, first.ticket);
    assert_eq!(
        open_titles(&conn),
        vec!["Build fails: rust-tests".to_owned()]
    );
}

#[test]
fn invalid_manifest_entries_file_one_ticket_per_family_and_message() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    // As an older daemon worded them, one report per check.
    let unresolved = |check: &str, why: &str| {
        report(
            check,
            IncompleteReason::Unreported,
            &format!("the inputs of {check} cannot be resolved: invalid: {why}"),
        )
    };
    let bench = unresolved(
        "allocations:bench",
        "crate-closure: the check belongs to no package",
    );
    let search = unresolved(
        "allocations:search",
        "crate-closure: the check belongs to no package",
    );
    let docs = unresolved("source:citations", "docs/*.json: matches no tracked file");
    let a = incomplete(&mut conn, UPKEEP, &bench, clock.now()).unwrap();
    let b = incomplete(&mut conn, UPKEEP, &search, clock.now()).unwrap();
    let c = incomplete(&mut conn, UPKEEP, &docs, clock.now()).unwrap();
    assert_eq!(a.ticket, b.ticket, "one family and message is one ticket");
    assert_ne!(a.ticket, c.ticket);
    let filed = ticket(&conn, &a.ticket);
    assert_eq!(filed.title, "Manifest entry is invalid: allocations");
    assert!(
        filed
            .body
            .starts_with("The manifest entry of allocations is invalid"),
        "{}",
        filed.body
    );
    // A new daemon names the reason and lists the checks in one report.
    let mut listed = report(
        "allocations:bench",
        IncompleteReason::InvalidEntry,
        "invalid: crate-closure: the check belongs to no package",
    );
    listed.commit = Commit::new("1963793e7").unwrap();
    listed.checks = vec![bench.check.clone(), search.check.clone()];
    let d = incomplete(&mut conn, UPKEEP, &listed, clock.now()).unwrap();
    assert_eq!((d.ticket, d.change), (a.ticket, Change::Commented));
}

#[test]
fn a_command_that_leaves_checks_unreported_files_one_ticket_per_family() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut one = report(
        "rust:a",
        IncompleteReason::Unreported,
        "thread 'main' panicked at src/lib.rs:4:5:\nharness died",
    );
    one.attempts = 3;
    let mut other = one.clone();
    other.check = CheckName::new("rust:b").unwrap();
    let a = incomplete(&mut conn, UPKEEP, &one, clock.now()).unwrap();
    let b = incomplete(&mut conn, UPKEEP, &other, clock.now()).unwrap();
    assert_eq!(a.ticket, b.ticket);
    let filed = ticket(&conn, &a.ticket);
    assert_eq!(filed.title, "Checks stay incomplete: rust");
    assert!(
        filed
            .body
            .starts_with("The command of rust ended without reporting 1 check in each of 3"),
        "{}",
        filed.body
    );
}

#[test]
fn a_run_with_30_causes_files_at_most_six_tickets() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    for i in 0..30 {
        let cause = report(
            &format!("markdown:doc{i}"),
            IncompleteReason::InvalidEntry,
            &format!("invalid: docs/part{i}/*.md: matches no tracked file"),
        );
        incomplete(&mut conn, UPKEEP, &cause, clock.now()).unwrap();
    }
    let filed = all_tickets(&conn, clock.now());
    assert_eq!(filed.len(), ROUND_LIMIT + 1, "{:?}", open_titles(&conn));
    let overflow = filed
        .iter()
        .find(|t| t.title.starts_with("Too many causes"))
        .expect("one ticket for the causes left out");
    assert!(overflow.body.contains("part5"), "{}", overflow.body);
    assert_eq!(
        comment_count(&conn, &overflow.id.to_string()),
        30 - ROUND_LIMIT - 1
    );

    // A cause that already has an open ticket still adds to it in a later run.
    let mut again = report(
        "markdown:doc0",
        IncompleteReason::InvalidEntry,
        "invalid: docs/part0/*.md: matches no tracked file",
    );
    again.commit = Commit::new("1963793e7").unwrap();
    assert_eq!(
        incomplete(&mut conn, UPKEEP, &again, clock.now())
            .unwrap()
            .change,
        Change::Commented
    );
}

/// A failed build that the tip waits on keeps `master` where it is, so its ticket is filed open
/// for the pool; one that holds back only checks run after `master` moved waits for a person.
#[test]
fn a_failed_build_is_filed_open_only_when_the_tip_waits_on_it() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut build = report("b3d:build", IncompleteReason::BuildFailed, "link failed");
    build.product = Some("b3d".to_owned());
    build.checks = vec![CheckName::new("b3d-test:faces").unwrap()];
    build.holds_tip = true;
    let held = incomplete(&mut conn, UPKEEP, &build, clock.now()).unwrap();
    assert_eq!(ticket(&conn, &held.ticket).status, Status::Open);

    let mut after = report("rust:build", IncompleteReason::BuildFailed, "link failed");
    after.product = Some("rust-tests".to_owned());
    let waits = incomplete(&mut conn, UPKEEP, &after, clock.now()).unwrap();
    assert_ne!(waits.ticket, held.ticket);
    assert_eq!(ticket(&conn, &waits.ticket).status, Status::PendingApproval);

    // A later tip that waits on the same build releases its ticket.
    after.commit = Commit::new("1963793e7").unwrap();
    after.holds_tip = true;
    let again = incomplete(&mut conn, UPKEEP, &after, clock.now()).unwrap();
    assert_eq!(
        (again.ticket.as_str(), again.change),
        (waits.ticket.as_str(), Change::Commented)
    );
    assert_eq!(ticket(&conn, &waits.ticket).status, Status::Open);
}
