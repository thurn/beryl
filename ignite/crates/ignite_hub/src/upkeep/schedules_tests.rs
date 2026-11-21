//! Meta-tickets filing their copies on upkeep passes.

use rusqlite::Connection;

use ignite_core::protocol::tickets::{Operation, Status, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub;
use ignite_store::hub::batch::{self, BatchOutcome};

use super::Upkeep;
use super::tests::{apply, start, ticket};

const HOUR: f64 = 3_600.0;

fn meta(fields: TicketFields) -> Operation {
    Operation::Create {
        fields: TicketFields {
            title: Some("Sweep stale tickets".to_owned()),
            body: Some("Run the sweep.".to_owned()),
            ticket_type: Some(TicketType::Meta),
            labels: Some(vec!["tier:sonnet".to_owned()]),
            priority: Some(1),
            every_hours: Some(Some(6)),
            ..fields
        },
        parent: None,
        deps: Vec::new(),
        request_key: None,
    }
}

fn pass(conn: &mut Connection, now: Timestamp) -> Vec<String> {
    Upkeep::new(None)
        .unwrap()
        .pass(conn, now)
        .unwrap()
        .tickets_filed
}

fn close(conn: &mut Connection, id: &str, now: Timestamp) {
    let op = Operation::Close {
        id: id.to_owned(),
        reason: "done".to_owned(),
        include_ready: false,
    };
    apply(conn, vec![op], now);
}

#[test]
fn a_meta_ticket_files_one_copy_at_a_time_on_its_clock() {
    let mut conn = hub::open_in_memory().unwrap();
    let t0 = start().now();
    let id = apply(&mut conn, vec![meta(TicketFields::default())], t0).remove(0);
    assert!(id.starts_with('M') && id.len() == 5, "{id}");
    assert_eq!(ticket(&conn, &id).status, Status::Open);

    let first = format!("{id}.1");
    assert_eq!(pass(&mut conn, t0), vec![first.clone()]);
    let copy = ticket(&conn, &first);
    assert_eq!(
        (
            copy.title.as_str(),
            copy.body.as_str(),
            copy.status,
            copy.priority
        ),
        ("Sweep stale tickets", "Run the sweep.", Status::Open, 1)
    );
    assert_eq!(copy.ticket_type, TicketType::Task);
    assert_eq!(copy.labels, vec!["tier:sonnet".to_owned()]);

    // The copy is live through the next due time, so nothing more is filed.
    assert!(pass(&mut conn, t0.plus_seconds(7.0 * HOUR)).is_empty());
    // Closing the copy leaves the meta-ticket open, and the due time at 6 hours was missed
    // while the copy was live, so the next copy waits for the one at 12 hours.
    close(&mut conn, &first, t0.plus_seconds(8.0 * HOUR));
    assert_eq!(ticket(&conn, &id).status, Status::Open);
    assert!(pass(&mut conn, t0.plus_seconds(11.9 * HOUR)).is_empty());
    assert_eq!(
        pass(&mut conn, t0.plus_seconds(12.0 * HOUR)),
        vec![format!("{id}.2")]
    );
}

#[test]
fn a_copy_starts_with_the_notes_on_the_meta_ticket_when_it_is_filed() {
    let mut conn = hub::open_in_memory().unwrap();
    let t0 = start().now();
    let id = apply(&mut conn, vec![meta(TicketFields::default())], t0).remove(0);
    let first = format!("{id}.1");
    assert_eq!(pass(&mut conn, t0), vec![first.clone()]);
    assert_eq!(ticket(&conn, &first).notes, "");

    // The first copy writes what it learned on the meta-ticket, then closes.
    let learned = "Round 1: the gate waited longest for a landing tree.";
    let note = Operation::Update {
        id: id.clone(),
        set: Some(TicketFields {
            notes: Some(learned.to_owned()),
            ..TicketFields::default()
        }),
        add_labels: Vec::new(),
        remove_labels: Vec::new(),
        set_meta: std::collections::BTreeMap::default(),
        unset_meta: Vec::new(),
    };
    apply(&mut conn, vec![note], t0.plus_seconds(HOUR));
    close(&mut conn, &first, t0.plus_seconds(HOUR));

    let second = format!("{id}.2");
    assert_eq!(
        pass(&mut conn, t0.plus_seconds(6.0 * HOUR)),
        vec![second.clone()]
    );
    assert_eq!(ticket(&conn, &second).notes, learned);
    // A copy already filed keeps the notes it was filed with.
    assert_eq!(ticket(&conn, &first).notes, "");
}

#[test]
fn a_deferred_meta_ticket_files_nothing() {
    let mut conn = hub::open_in_memory().unwrap();
    let t0 = start().now();
    let paused = TicketFields {
        status: Some(Status::Deferred),
        ..TicketFields::default()
    };
    apply(&mut conn, vec![meta(paused)], t0);
    assert!(pass(&mut conn, t0.plus_seconds(24.0 * HOUR)).is_empty());
}

#[test]
fn a_meta_ticket_needs_a_schedule_and_no_parent() {
    let mut conn = hub::open_in_memory().unwrap();
    let t0 = start().now();
    let refused = |conn: &mut Connection, op: Operation| {
        let request = ignite_core::protocol::tickets::BatchRequest {
            preconditions: Vec::new(),
            ops: vec![op],
        };
        !matches!(
            batch::apply(conn, &request, "test", "m-1", t0),
            Ok(BatchOutcome::Applied(_))
        )
    };
    let Operation::Create { fields, .. } = meta(TicketFields::default()) else {
        unreachable!()
    };
    assert!(refused(
        &mut conn,
        Operation::Create {
            fields: TicketFields {
                every_hours: None,
                ..fields
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        }
    ));
    let root = apply(&mut conn, vec![meta(TicketFields::default())], t0).remove(0);
    let Operation::Create { fields, .. } = meta(TicketFields::default()) else {
        unreachable!()
    };
    assert!(refused(
        &mut conn,
        Operation::Create {
            fields,
            parent: Some(root.clone()),
            deps: Vec::new(),
            request_key: None,
        }
    ));
    // A meta-ticket cannot turn into an ordinary task, whose ID would start with `T`.
    let retype = Operation::Update {
        id: root,
        set: Some(TicketFields {
            ticket_type: Some(TicketType::Task),
            ..TicketFields::default()
        }),
        add_labels: Vec::new(),
        remove_labels: Vec::new(),
        set_meta: std::collections::BTreeMap::default(),
        unset_meta: Vec::new(),
    };
    assert!(refused(&mut conn, retype));
}
