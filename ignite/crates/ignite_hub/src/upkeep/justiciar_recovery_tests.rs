use ignite_core::fakes::FakeClock;
use ignite_core::protocol::hub::{JusticiarCleared, JusticiarRecovery, JusticiarReport};
use ignite_core::protocol::tickets::{Operation, Status};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub;

use super::clearing::{HOLD_SECONDS, RETURNED_HOLD_SECONDS, close_cleared};
use super::filing::{Change, UPKEEP};
use super::justiciar;
use super::tests::{apply, comment_count, ticket};

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
