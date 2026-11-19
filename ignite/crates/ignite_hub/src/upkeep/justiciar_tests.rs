//! Justiciar tickets from reports and from a `master` that stopped moving.

use ignite_core::ids::MachineId;
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::protocol::tickets::{Status, TicketType};
use ignite_core::traits::Clock;
use ignite_store::hub;

use super::filing::{Change, UPKEEP};
use super::justiciar::{self, MASTER_LAG_SECONDS};
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
    assert_eq!(
        ticket(&conn, &a.ticket).owner,
        Some("jarvis".parse().unwrap())
    );
    assert_eq!(
        ticket(&conn, &b.ticket).owner,
        Some("cygnus".parse().unwrap())
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
fn an_open_regression_ticket_explains_the_lag_and_files_nothing() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    landing(&mut conn, &clock);
    let regression = apply(
        &mut conn,
        vec![create("pytest fails", TicketType::Bug, None, &[])],
        clock.now(),
    );
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('pytest', ?1, 'bbbbbbb', 'aaaaaaa', 'open')",
        [&regression[0]],
    )
    .unwrap();
    clock.advance(MASTER_LAG_SECONDS + 60.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );

    // Once the regression ticket is closed, a lag that remains is no longer explained.
    close(&mut conn, &regression[0], &clock.now().to_string());
    assert_eq!(
        justiciar::master_lag(&mut conn, clock.now()).unwrap().len(),
        1
    );
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
