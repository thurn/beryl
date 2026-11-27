use ignite_core::fakes::FakeClock;
use ignite_core::protocol::hub::{JusticiarCleared, JusticiarRecovery, JusticiarReport};
use ignite_core::protocol::tickets::{Operation, Status, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub;
use rusqlite::{Connection, params};

use super::clearing::{HOLD_SECONDS, RETURNED_HOLD_SECONDS, close_cleared};
use super::filing::{Change, UPKEEP};
use super::justiciar::{self, MASTER_LAG_SECONDS};
use super::regression_stalls::{FALLBACK_AGE_SECONDS, INACTIVE_SECONDS};
use super::tests::{apply, comment_count, create, start, ticket};

fn report(subject: &str, evidence: String) -> JusticiarReport {
    JusticiarReport {
        subject: subject.to_owned(),
        title: "Emergency".to_owned(),
        details: String::new(),
        evidence,
        machine: "m-1".parse().unwrap(),
        host: "one".to_owned(),
        on_machine: true,
    }
}

#[test]
fn the_reporting_machine_clears_a_loop_and_a_new_failure_reopens_its_ticket() {
    let clock = FakeClock::new(Timestamp::EPOCH);
    let mut conn = hub::open_in_memory().unwrap();
    let mut fault = report("loop:mirror", "old-daemon-evidence".to_owned());
    let filed = justiciar::report(&mut conn, UPKEEP, &fault, clock.now()).unwrap();
    clock.advance(1.0);
    let mut cleared = JusticiarCleared {
        subject: fault.subject.clone(),
        recovery: JusticiarRecovery::LoopRound {
            finished_at: clock.now(),
        },
    };
    justiciar::cleared(&mut conn, &"m-2".parse().unwrap(), &cleared, clock.now()).unwrap();
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(HOLD_SECONDS);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());

    justiciar::cleared(&mut conn, &fault.machine, &cleared, clock.now()).unwrap();
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(HOLD_SECONDS - 1.0);
    justiciar::cleared(&mut conn, &fault.machine, &cleared, clock.now()).unwrap();
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(1.0);
    assert_eq!(
        close_cleared(&mut conn, clock.now()).unwrap(),
        std::slice::from_ref(&filed.ticket)
    );

    fault.evidence = format!("{}|next-daemon-run", clock.now());
    let returned = justiciar::report(&mut conn, UPKEEP, &fault, clock.now()).unwrap();
    assert_eq!(returned.change, Change::Reopened);
    assert_eq!(returned.ticket, filed.ticket);
    justiciar::cleared(&mut conn, &fault.machine, &cleared, clock.now()).unwrap();
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(RETURNED_HOLD_SECONDS);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());

    cleared.recovery = JusticiarRecovery::LoopRound {
        finished_at: clock.now(),
    };
    justiciar::cleared(&mut conn, &fault.machine, &cleared, clock.now()).unwrap();
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(HOLD_SECONDS);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(RETURNED_HOLD_SECONDS - HOLD_SECONDS);
    assert_eq!(
        close_cleared(&mut conn, clock.now()).unwrap(),
        [filed.ticket]
    );
}

#[test]
fn recovery_does_not_close_a_persons_ticket_or_erase_a_later_sighting() {
    let clock = FakeClock::new(Timestamp::EPOCH);
    let mut conn = hub::open_in_memory().unwrap();
    let mut fault = report("crash:startup:", "first.json".to_owned());
    let crash = justiciar::report(&mut conn, UPKEEP, &fault, clock.now())
        .unwrap()
        .ticket;
    let request = report("request:worker", "request".to_owned());
    let manual = justiciar::report(&mut conn, UPKEEP, &request, clock.now())
        .unwrap()
        .ticket;
    let idle = report("idle:audit", "audit@date".to_owned());
    let touched = justiciar::report(&mut conn, UPKEEP, &idle, clock.now())
        .unwrap()
        .ticket;
    apply(
        &mut conn,
        vec![Operation::Comment {
            id: touched.clone(),
            text: "Investigating".to_owned(),
        }],
        clock.now(),
    );

    let clear = |conn: &mut _, report: &JusticiarReport| {
        let message = JusticiarCleared {
            subject: report.subject.clone(),
            recovery: JusticiarRecovery::Sighting {
                evidence: report.evidence.clone(),
            },
        };
        justiciar::cleared(conn, &report.machine, &message, clock.now()).unwrap();
    };
    clear(&mut conn, &fault);
    clear(&mut conn, &request);
    clear(&mut conn, &idle);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    let old = fault.clone();
    fault.evidence = "another-crash.json".to_owned();
    justiciar::report(&mut conn, UPKEEP, &fault, clock.now()).unwrap();
    clear(&mut conn, &old);

    clock.advance(2.0 * HOLD_SECONDS);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    assert_eq!(ticket(&conn, &crash).status, Status::Open);
    assert_eq!(ticket(&conn, &manual).status, Status::Open);
    assert_eq!(ticket(&conn, &touched).status, Status::Open);
    assert_eq!(comment_count(&conn, &touched), 1);

    clear(&mut conn, &fault);
    assert!(close_cleared(&mut conn, clock.now()).unwrap().is_empty());
    clock.advance(HOLD_SECONDS);
    assert_eq!(close_cleared(&mut conn, clock.now()).unwrap(), [crash]);
    assert_eq!(ticket(&conn, &manual).status, Status::Open);
    assert_eq!(ticket(&conn, &touched).status, Status::Open);
}

#[test]
fn a_failed_tip_after_a_regression_fix_ends_the_verification_wait() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let landed = apply(
        &mut conn,
        vec![create("work", TicketType::Task, None, &[])],
        clock.now(),
    );
    conn.execute(
        "UPDATE tickets SET landed_at = ?1 WHERE id = ?2",
        [clock.now().to_string(), landed[0].clone()],
    )
    .unwrap();
    clock.advance(MASTER_LAG_SECONDS + 60.0);

    // An earlier tip failed before the regression fix landed.
    let earlier_fail = clock.now().to_string();
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, verified, state) VALUES ('aaaa111', ?1, ?1, 'failed')",
        [&earlier_fail],
    )
    .unwrap();
    clock.advance(60.0);

    let reg = apply(
        &mut conn,
        vec![create("pytest fails", TicketType::Bug, None, &[])],
        clock.now(),
    );
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('quality:unit', ?1, 'aaaa111', '0000000', 'open')",
        [&reg[0]],
    )
    .unwrap();
    let fix_at = clock.now().to_string();
    conn.execute(
        "UPDATE tickets SET landed_at = ?1, status = 'closed', closed_at = ?1 WHERE id = ?2",
        [&fix_at, &reg[0]],
    )
    .unwrap();

    // Still awaiting verification: the earlier failure predates the fix.
    clock.advance(60.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );

    // A tip seen after the fix landed fails, so the fix did not end the stall.
    let later_fail = clock.now().to_string();
    conn.execute(
        "INSERT INTO tips (commit_sha, seen, verified, state) VALUES ('bbbb222', ?1, ?1, 'failed')",
        [&later_fail],
    )
    .unwrap();
    clock.advance(60.0);
    let filed = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    assert!(
        ticket(&conn, &filed[0].ticket)
            .title
            .starts_with("master has not moved")
    );
}

fn setup_old_regression_stall(
    conn: &mut Connection,
    clock: &FakeClock,
) -> (String, String, String) {
    let landed = apply(
        conn,
        vec![create("landed work", TicketType::Task, None, &[])],
        clock.now(),
    );
    conn.execute(
        "UPDATE tickets SET landed_at = ?2 WHERE id = ?1",
        params![landed[0], clock.now().to_string()],
    )
    .unwrap();
    let regression = apply(
        conn,
        vec![create("pytest fails", TicketType::Bug, None, &[])],
        clock.now(),
    )[0]
    .clone();
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('quality:unit', ?1, 'bbbbbbb', 'aaaaaaa', 'open')",
        [&regression],
    )
    .unwrap();
    clock.advance(FALLBACK_AGE_SECONDS);
    apply(
        conn,
        vec![Operation::Claim {
            id: regression.clone(),
        }],
        clock.now(),
    );
    let holder: String = conn
        .query_row(
            "SELECT assignee FROM tickets WHERE id = ?1",
            [&regression],
            |row| row.get(0),
        )
        .unwrap();
    let filed = justiciar::master_lag(conn, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    let escalation = filed[0].ticket.clone();
    assert!(
        ticket(conn, &escalation)
            .body
            .contains("historical 90th-percentile")
    );
    (regression, holder, escalation)
}

#[test]
fn a_closed_regression_stall_waits_for_the_same_reason_and_escalates_on_a_new_one() {
    let clock = FakeClock::new(Timestamp::parse("2026-06-01T00:00:00Z").unwrap());
    let mut conn = hub::open_in_memory().unwrap();
    let (_regression, _holder, escalation) = setup_old_regression_stall(&mut conn, &clock);

    apply(
        &mut conn,
        vec![Operation::Close {
            id: escalation.clone(),
            reason: "Holder is actively working".to_owned(),
            include_ready: false,
        }],
        clock.now(),
    );

    // Same Stall::Old reason is suppressed immediately after the worker closes the escalation.
    clock.advance(10.0);
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );

    // A different stall reason (Inactive) escalates right away without waiting for the grace window.
    clock.advance(INACTIVE_SECONDS);
    let filed = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    assert_ne!(filed[0].ticket, escalation);
    assert!(
        ticket(&conn, &filed[0].ticket)
            .body
            .contains("last recorded activity")
    );
}

#[test]
fn a_closed_regression_stall_refiles_after_the_grace_window() {
    let clock = FakeClock::new(Timestamp::parse("2026-06-01T00:00:00Z").unwrap());
    let mut conn = hub::open_in_memory().unwrap();
    let (regression, holder, escalation) = setup_old_regression_stall(&mut conn, &clock);

    apply(
        &mut conn,
        vec![Operation::Close {
            id: escalation.clone(),
            reason: "Holder is actively working".to_owned(),
            include_ready: false,
        }],
        clock.now(),
    );

    // Keep the holder active past the grace window so the stall remains Stall::Old.
    clock.advance(MASTER_LAG_SECONDS - 10.0);
    conn.execute(
        "INSERT INTO records (ticket, session, at, verb, machine, class, queued_s, wall_s, cpu_s,
                              peak_mb, outcome, ignite)
         VALUES (?1, ?2, ?3, 'test', 'machine', 'agent', 0, 0, 0, 0, 'pass', 'test')",
        params![regression, holder, clock.now().to_string()],
    )
    .unwrap();
    assert!(
        justiciar::master_lag(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );

    clock.advance(10.0);
    let filed = justiciar::master_lag(&mut conn, clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].change, Change::Created);
    assert_ne!(filed[0].ticket, escalation);
    assert!(
        ticket(&conn, &filed[0].ticket)
            .body
            .contains("historical 90th-percentile")
    );
}
