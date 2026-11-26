//! Upkeep switches off a check that holds `master` back without a record of catching a defect.

use ignite_core::config::Rules;
use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId};
use ignite_core::protocol::hub::{ResultKey, TipFailure, TipState, TipVerified};
use ignite_core::protocol::tickets::TicketType;
use ignite_core::traits::Clock;
use ignite_store::hub::{self, tickets, tips};
use rusqlite::{Connection, params};

use super::tests::{apply, check_state, create, list_check, start};
use super::unproven_holds::{self, HOLD_SECONDS};

fn rules() -> Rules {
    Rules {
        protected: vec!["clippy".to_owned()],
        budgets: vec!["counts".to_owned()],
        ..Rules::default()
    }
}

fn verified(conn: &Connection, commit: &str, clock: &FakeClock) {
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, verified, state) VALUES (?1, ?2, ?2, 'verified')",
        params![commit, clock.now().to_string()],
    )
    .unwrap();
}

fn failed(conn: &Connection, commit: &str, checks: &[&str], clock: &FakeClock) {
    tips::verified(
        conn,
        &TipVerified {
            commit: Commit::new(commit).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Failed,
            regression_passes: None,
            failed_descendant: None,
            failure: Some(TipFailure {
                checks: checks
                    .iter()
                    .map(|check| ResultKey {
                        check: CheckName::new(*check).unwrap(),
                        hash: InputHash::new("a".repeat(64)).unwrap(),
                        platform: None,
                    })
                    .collect(),
                unverified_commits: vec![Commit::new(commit).unwrap()],
            }),
        },
        clock.now(),
    )
    .unwrap();
}

fn fixed_before(conn: &Connection, check: &str, outcome: &str) {
    conn.execute(
        "INSERT INTO check_fix_outcomes (close_seq, check_name, ticket, at, outcome, source)
         VALUES (1, ?1, ?2, '2026-11-01T00:00:00Z', ?3, 'regression')",
        params![check, format!("old-{check}"), outcome],
    )
    .unwrap();
}

fn regression_ticket(conn: &mut Connection, check: &str, clock: &FakeClock) -> String {
    let id = apply(
        conn,
        vec![create("Regression", TicketType::Bug, None, &[])],
        clock.now(),
    )
    .remove(0);
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES (?1, ?2, 'bbbb', 'aaaa', 'open')",
        params![check, id],
    )
    .unwrap();
    id
}

fn pass(conn: &mut Connection, clock: &FakeClock) -> Vec<String> {
    unproven_holds::switch_off(conn, &rules(), clock.now()).unwrap()
}

#[test]
fn an_unproven_check_holding_master_for_an_hour_is_switched_off_and_its_ticket_told() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    for check in [
        "rust:frames",
        "rust:caught",
        "clippy:lint",
        "counts:rollouts",
    ] {
        list_check(&mut conn, check);
    }
    fixed_before(&conn, "rust:frames", "test-only");
    fixed_before(&conn, "rust:caught", "product");
    // A count fix that got back under the limit changed product files, but caught no defect.
    fixed_before(&conn, "counts:rollouts", "product");
    let ticket = regression_ticket(&mut conn, "rust:frames", &clock);
    verified(&conn, "aaaa", &clock);
    clock.advance(60.0);
    let checks = [
        "rust:frames",
        "rust:caught",
        "clippy:lint",
        "counts:rollouts",
    ];
    failed(&conn, "bbbb", &checks, &clock);

    clock.advance(HOLD_SECONDS - 60.0);
    assert!(
        pass(&mut conn, &clock).is_empty(),
        "the hold is not an hour old yet"
    );

    clock.advance(120.0);
    assert_eq!(pass(&mut conn, &clock), ["counts:rollouts", "rust:frames"]);
    assert_eq!(check_state(&conn, "rust:frames"), "off");
    assert_eq!(check_state(&conn, "rust:caught"), "on");
    assert_eq!(check_state(&conn, "clippy:lint"), "on");
    let comments = tickets::comments(&conn, &ticket).unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].text.contains("ignite flaky on rust:frames"));
    let recorded: (String, Option<String>) = conn
        .query_row(
            "SELECT actor, ticket FROM check_state_changes WHERE check_name = 'rust:frames'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(recorded, ("ignite-upkeep".to_owned(), Some(ticket)));

    assert!(
        pass(&mut conn, &clock).is_empty(),
        "a check is switched off once"
    );
}

#[test]
fn a_cross_family_product_close_does_not_protect_an_unproven_check() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    for check in ["rust:tainted", "pytest:other", "rust:same-a", "rust:same-b"] {
        list_check(&mut conn, check);
    }
    // Cross-family bulk/reopened close: rust:tainted rode along with pytest:other.
    for check in ["rust:tainted", "pytest:other"] {
        conn.execute(
            "INSERT INTO check_fix_outcomes (close_seq, check_name, ticket, at, outcome, source)
             VALUES (10, ?1, 'multi', '2026-11-01T00:00:00Z', 'product', 'regression')",
            params![check],
        )
        .unwrap();
    }
    // Single-family close: two checks in the same family fixed together still count.
    for check in ["rust:same-a", "rust:same-b"] {
        conn.execute(
            "INSERT INTO check_fix_outcomes (close_seq, check_name, ticket, at, outcome, source)
             VALUES (11, ?1, 'single-fam', '2026-11-01T00:00:00Z', 'product', 'regression')",
            params![check],
        )
        .unwrap();
    }
    verified(&conn, "aaaa", &clock);
    clock.advance(60.0);
    failed(&conn, "bbbb", &["rust:tainted", "rust:same-a"], &clock);
    clock.advance(HOLD_SECONDS + 60.0);

    assert_eq!(pass(&mut conn, &clock), ["rust:tainted"]);
    assert_eq!(check_state(&conn, "rust:tainted"), "off");
    assert_eq!(check_state(&conn, "rust:same-a"), "on");
}

#[test]
fn a_failure_before_the_newest_verified_tip_holds_nothing() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    list_check(&mut conn, "rust:frames");
    failed(&conn, "aaaa", &["rust:frames"], &clock);
    clock.advance(60.0);
    verified(&conn, "bbbb", &clock);
    clock.advance(2.0 * HOLD_SECONDS);
    assert!(pass(&mut conn, &clock).is_empty());
    assert_eq!(check_state(&conn, "rust:frames"), "on");
}

fn observed_superseded(conn: &Connection, commit: &str, checks: &[&str], clock: &FakeClock) {
    let keys: Vec<ResultKey> = checks
        .iter()
        .map(|check| ResultKey {
            check: CheckName::new(*check).unwrap(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform: None,
        })
        .collect();
    let observed = serde_json::to_string(&keys).unwrap();
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, state, observed_failures)
         VALUES (?1, ?2, 'superseded', ?3)",
        params![commit, clock.now().to_string(), observed],
    )
    .unwrap();
}

#[test]
fn hold_duration_starts_when_the_first_failing_tip_was_seen() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    list_check(&mut conn, "counts:family/attachments");
    verified(&conn, "aaaa", &clock);
    clock.advance(60.0);
    // The tip lands at t0, but its slow round finishes failing 40 minutes later.
    let seen = clock.now();
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, state) VALUES ('bbbb', ?1, 'verifying')",
        params![seen.to_string()],
    )
    .unwrap();
    clock.advance(2400.0);
    failed(&conn, "bbbb", &["counts:family/attachments"], &clock);

    // 20 minutes after failure (60 minutes after the tip landed), the hold has reached an hour.
    clock.advance(HOLD_SECONDS - 2400.0);
    assert_eq!(pass(&mut conn, &clock), ["counts:family/attachments"]);
    assert_eq!(check_state(&conn, "counts:family/attachments"), "off");
}

#[test]
fn repeated_observed_failures_on_superseded_tips_switch_off_but_a_single_flake_does_not() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    list_check(&mut conn, "counts:rollouts");
    list_check(&mut conn, "rust:flake");
    verified(&conn, "aaaa", &clock);
    clock.advance(60.0);
    observed_superseded(&conn, "bbbb", &["counts:rollouts", "rust:flake"], &clock);
    clock.advance(600.0);
    observed_superseded(&conn, "cccc", &["counts:rollouts"], &clock);

    clock.advance(HOLD_SECONDS - 660.0);
    assert!(
        pass(&mut conn, &clock).is_empty(),
        "an hour has not elapsed since the first superseded tip was seen"
    );

    clock.advance(60.0);
    assert_eq!(pass(&mut conn, &clock), ["counts:rollouts"]);
    assert_eq!(check_state(&conn, "counts:rollouts"), "off");
    assert_eq!(check_state(&conn, "rust:flake"), "on");
}

#[test]
fn a_check_re_enabled_after_a_failing_tip_is_not_switched_off_for_that_older_tip() {
    use ignite_core::protocol::hub::CheckState;
    use ignite_store::hub::check_state::{self, Switch};

    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    list_check(&mut conn, "counts:family/attachments");
    verified(&conn, "aaaa", &clock);
    clock.advance(60.0);
    failed(&conn, "bbbb", &["counts:family/attachments"], &clock);

    // After an hour the check is switched off, then a worker fixes it and switches it back on.
    clock.advance(HOLD_SECONDS + 60.0);
    assert_eq!(pass(&mut conn, &clock), ["counts:family/attachments"]);
    assert_eq!(check_state(&conn, "counts:family/attachments"), "off");

    clock.advance(60.0);
    check_state::switch(
        &conn,
        &Switch {
            check: "counts:family/attachments",
            state: CheckState::On,
            actor: "worker",
            machine: "m-1",
            note: "fixed",
            ticket: None,
            at: clock.now(),
        },
    )
    .unwrap();

    // The older failing tip predates the re-enable, so upkeep leaves the check on.
    clock.advance(30.0);
    assert!(pass(&mut conn, &clock).is_empty());
    assert_eq!(check_state(&conn, "counts:family/attachments"), "on");

    // A new failing tip seen after the re-enable starts a fresh hour-long hold.
    failed(&conn, "cccc", &["counts:family/attachments"], &clock);
    clock.advance(HOLD_SECONDS - 60.0);
    assert!(pass(&mut conn, &clock).is_empty());
    clock.advance(120.0);
    assert_eq!(pass(&mut conn, &clock), ["counts:family/attachments"]);
    assert_eq!(check_state(&conn, "counts:family/attachments"), "off");
}

#[test]
fn a_check_that_passes_on_a_newer_failed_tip_clears_its_hold() {
    let mut conn = hub::open_in_memory().unwrap();
    let clock = start();
    list_check(&mut conn, "pytest:inputs");
    list_check(&mut conn, "rust:other");
    verified(&conn, "aaaa", &clock);
    clock.advance(60.0);
    failed(&conn, "bbbb", &["pytest:inputs"], &clock);

    // A newer tip fixes pytest:inputs (recording it in regression_passes) while failing rust:other.
    clock.advance(600.0);
    tips::verified(
        &conn,
        &TipVerified {
            commit: Commit::new("cccc").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Failed,
            regression_passes: Some(vec![ResultKey {
                check: CheckName::new("pytest:inputs").unwrap(),
                hash: InputHash::new("b".repeat(64)).unwrap(),
                platform: None,
            }]),
            failed_descendant: None,
            failure: Some(TipFailure {
                checks: vec![ResultKey {
                    check: CheckName::new("rust:other").unwrap(),
                    hash: InputHash::new("c".repeat(64)).unwrap(),
                    platform: None,
                }],
                unverified_commits: vec![Commit::new("cccc").unwrap()],
            }),
        },
        clock.now(),
    )
    .unwrap();

    // Past an hour from bbbb (and before an hour from cccc), neither check is switched off.
    clock.advance(HOLD_SECONDS - 500.0);
    assert!(pass(&mut conn, &clock).is_empty());
    assert_eq!(check_state(&conn, "pytest:inputs"), "on");

    // Past an hour from cccc, only rust:other is switched off; pytest:inputs stays on.
    clock.advance(600.0);
    assert_eq!(pass(&mut conn, &clock), ["rust:other"]);
    assert_eq!(check_state(&conn, "pytest:inputs"), "on");
    assert_eq!(check_state(&conn, "rust:other"), "off");
}
