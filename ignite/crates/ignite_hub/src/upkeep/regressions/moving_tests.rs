use rusqlite::Connection;

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::protocol::tickets::{Operation, Status, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::StoreError;
use ignite_store::hub::{self, tickets};

use super::super::{ADDED_KEY, Outcome, file};
use super::{Moved, move_check};
use crate::upkeep::filing::{Author, UPKEEP};
use crate::upkeep::tests::{apply, create, start, ticket};

const WORKER: Author = Author {
    actor: "local_w",
    machine: "m-1",
};

fn failing(check: &str, first_bad: &str) -> RegressionReport {
    RegressionReport {
        check: CheckName::new(check).unwrap(),
        first_bad: Commit::new(first_bad).unwrap(),
        last_good: Commit::new("0000").unwrap(),
        caused_by: None,
        output: "assertion failed".to_owned(),
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by: FoundBy::TipVerification,
        detected_at: None,
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

fn name(check: &str) -> CheckName {
    CheckName::new(check).unwrap()
}

/// Regression ticket ANSGS held `master` back for hours: the hub had grouped a second check
/// onto it, a worker split that check out into a ticket of its own, but the hub still mapped the
/// check to ANSGS, so every later failure of it reopened ANSGS.
#[test]
fn a_moved_check_fails_again_and_reopens_its_new_ticket_not_the_source() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let (source, _) = file(
        &mut conn,
        UPKEEP,
        &failing("rust:a::one", "1111"),
        clock.now(),
    )
    .unwrap();
    clock.advance(60.0);
    let (grouped, outcome) = file(
        &mut conn,
        UPKEEP,
        &failing("counts:rollouts", "2222"),
        clock.now(),
    )
    .unwrap();
    assert_eq!(
        (grouped.as_str(), outcome),
        (source.as_str(), Outcome::Added)
    );

    clock.advance(60.0);
    let split = apply(
        &mut conn,
        vec![create("Fix rollouts", TicketType::Bug, None, &[])],
        clock.now(),
    )
    .remove(0);
    clock.advance(60.0);
    let moved = move_check(
        &mut conn,
        WORKER,
        &name("counts:rollouts"),
        &split,
        clock.now(),
    )
    .unwrap();
    assert_eq!(moved.from.as_str(), source);
    assert_eq!(moved.to.as_str(), split);
    let target = ticket(&conn, &split);
    assert!(target.labels.contains(&"regression".to_owned()));
    assert!(target.labels.contains(&"check:counts".to_owned()));
    assert_eq!(
        target.metadata[ADDED_KEY],
        serde_json::json!(clock.now().to_string())
    );
    let told = tickets::comments(&conn, &source).unwrap();
    assert!(
        told.last()
            .unwrap()
            .text
            .contains("its other checks pass: rust:a::one")
    );

    // Moving it again to where it already is changes nothing.
    let comments = tickets::comments(&conn, &split).unwrap().len();
    let again = move_check(
        &mut conn,
        WORKER,
        &name("counts:rollouts"),
        &split,
        clock.now(),
    );
    let already = Moved {
        from: moved.to.clone(),
        to: moved.to,
    };
    assert_eq!(again.unwrap(), already);
    assert_eq!(tickets::comments(&conn, &split).unwrap().len(), comments);

    // The source's fix lands, but the moved check still fails: the source stays closed.
    clock.advance(60.0);
    close(&mut conn, &source, clock.now());
    clock.advance(60.0);
    let (filed, outcome) = file(
        &mut conn,
        UPKEEP,
        &failing("counts:rollouts", "3333"),
        clock.now(),
    )
    .unwrap();
    assert_eq!(
        (filed.as_str(), outcome),
        (split.as_str(), Outcome::Unchanged)
    );
    assert_eq!(ticket(&conn, &source).status, Status::Closed);

    // After the new ticket closes, a later failure reopens it and not the source.
    clock.advance(60.0);
    close(&mut conn, &split, clock.now());
    clock.advance(60.0);
    let (filed, outcome) = file(
        &mut conn,
        UPKEEP,
        &failing("counts:rollouts", "4444"),
        clock.now(),
    )
    .unwrap();
    assert_eq!(
        (filed.as_str(), outcome),
        (split.as_str(), Outcome::Reopened)
    );
    assert_eq!(ticket(&conn, &split).status, Status::Open);
    assert_eq!(ticket(&conn, &source).status, Status::Closed);
}

#[test]
fn a_grouped_check_reopening_a_regression_updates_the_title_and_body() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let (id, _) = file(
        &mut conn,
        UPKEEP,
        &failing("rust:a::one", "1111"),
        clock.now(),
    )
    .unwrap();
    let mut grouped = failing("source:file-length", "1111");
    let (same, outcome) = file(&mut conn, UPKEEP, &grouped, clock.now()).unwrap();
    assert_eq!((same.as_str(), outcome), (id.as_str(), Outcome::Added));
    clock.advance(600.0);
    close(&mut conn, &id, clock.now());
    clock.advance(600.0);
    grouped.first_bad = Commit::new("2222").unwrap();
    grouped.output = "file.rs: 801 lines".to_owned();
    let (reopened_id, outcome) = file(&mut conn, UPKEEP, &grouped, clock.now()).unwrap();
    assert_eq!(
        (reopened_id.as_str(), outcome),
        (id.as_str(), Outcome::Reopened)
    );
    let reopened = ticket(&conn, &id);
    assert_eq!(reopened.title, "Regression: source:file-length");
    assert!(
        reopened
            .body
            .contains("rerun: ignite test source:file-length")
    );
    assert!(reopened.body.contains("file.rs: 801 lines"));
}

#[test]
fn a_move_is_refused_without_a_regression_or_onto_a_closed_ticket() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let (source, _) = file(
        &mut conn,
        UPKEEP,
        &failing("rust:a::one", "1111"),
        clock.now(),
    )
    .unwrap();
    let other = apply(
        &mut conn,
        vec![create("Other", TicketType::Bug, None, &[])],
        clock.now(),
    )
    .remove(0);
    let refused = |result: Result<Moved, StoreError>| match result {
        Err(StoreError::Rejected(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    };

    let unknown = move_check(
        &mut conn,
        WORKER,
        &name("rust:a::none"),
        &other,
        clock.now(),
    );
    assert!(refused(unknown).contains("has no regression ticket"));

    close(&mut conn, &other, clock.now());
    let closed = move_check(&mut conn, WORKER, &name("rust:a::one"), &other, clock.now());
    assert!(refused(closed).contains("is closed"));
    let still: String = conn
        .query_row(
            "SELECT ticket FROM regressions WHERE check_name = 'rust:a::one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still, source);
}
