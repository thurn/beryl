//! Closing an epic's last descendant: an epic with an acceptance gets a child that verifies it
//! instead of closing, and closes when that child does; an epic without one closes at once.
// The shared helpers in `common` take JSON by value, as every test file that uses them does.
#![allow(clippy::needless_pass_by_value)]

mod common;

use serde_json::json;

use common::{Hub, create};
use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{OperationResult, Status, TicketType};

fn close(hub: &mut Hub, id: &str) -> OperationResult {
    hub.batch(json!([{"op": "close", "id": id, "reason": "Done"}]))
        .results
        .remove(0)
}

fn names(ids: &[TicketId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

#[test]
fn the_last_child_of_an_epic_with_an_acceptance_files_a_verify_child() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create(
            "Fast",
            None,
            json!({"type": "epic", "priority": 1, "acceptance": "Lands in 2 minutes."})
        ),
        create("Part", Some("$0"), json!({"labels": ["tier:sonnet"]})),
        create("Design", Some("$0"), json!({"labels": ["tier:opus", "x"]})),
    ]));
    let [epic, part, design] = [&ids[0], &ids[1], &ids[2]];
    close(&mut hub, part);
    let last = close(&mut hub, design);

    assert!(last.closed.is_empty());
    let verify = format!("{epic}.3");
    assert_eq!(names(&last.filed), [verify.as_str()]);
    assert_eq!(hub.show(epic).ticket.status, Status::Open);
    let shown = hub.show(&verify).ticket;
    assert_eq!(shown.title, format!("Verify {epic}'s acceptance"));
    assert_eq!(shown.status, Status::Open);
    assert_eq!(shown.ticket_type, TicketType::Task);
    assert_eq!(shown.priority, 1);
    assert_eq!(shown.labels, ["tier:opus"]);
    assert!(
        shown.body.ends_with("Lands in 2 minutes."),
        "{}",
        shown.body
    );
    assert_eq!(
        hub.list(json!({"ready": true, "ancestor": epic})),
        [verify.as_str()]
    );

    // The verify session files missing work under the epic, then closes its own ticket.
    let missing = hub
        .ids(json!([create("Missing", Some(epic.as_str()), json!({}))]))
        .remove(0);
    let verified = close(&mut hub, &verify);
    assert!(verified.closed.is_empty() && verified.filed.is_empty());
    // The missing work closing asks for the acceptance to be measured again.
    let refiled = close(&mut hub, &missing);
    let second = format!("{epic}.5");
    assert_eq!(names(&refiled.filed), [second.as_str()]);
    assert_eq!(hub.show(epic).ticket.status, Status::Open);

    // A verify ticket closing with nothing else live closes the epic.
    let done = close(&mut hub, &second);
    assert_eq!(names(&done.closed), [epic.as_str()]);
    assert!(done.filed.is_empty());
    assert_eq!(hub.show(epic).ticket.status, Status::Closed);
}

#[test]
fn an_epic_without_an_acceptance_closes_with_its_last_child() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create(
            "Outer",
            None,
            json!({"type": "epic", "acceptance": "Measured."})
        ),
        create(
            "Inner",
            Some("$0"),
            json!({"type": "epic", "acceptance": "  "})
        ),
        create("Task", Some("$1"), json!({})),
    ]));
    let [outer, inner, task] = [&ids[0], &ids[1], &ids[2]];
    let closed = close(&mut hub, task);
    // The inner epic closes; the outer one, which has an acceptance, is verified instead.
    assert_eq!(names(&closed.closed), [inner.as_str()]);
    assert_eq!(names(&closed.filed), [format!("{outer}.2")]);
    assert_eq!(hub.show(outer).ticket.status, Status::Open);
}

#[test]
fn reopening_a_child_makes_an_open_verify_child_wait_on_it() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create(
            "Epic",
            None,
            json!({"type": "epic", "acceptance": "Measured."})
        ),
        create("Child", Some("$0"), json!({})),
    ]));
    let [epic, child] = [&ids[0], &ids[1]];

    let closed = close(&mut hub, child);
    let verify = format!("{epic}.2");
    assert_eq!(names(&closed.filed), [verify.as_str()]);
    assert_eq!(
        hub.list(json!({"ready": true, "ancestor": epic})),
        [verify.as_str()]
    );

    hub.batch(json!([{"op": "reopen", "id": child}]));
    assert_eq!(
        hub.list(json!({"ready": true, "ancestor": epic})),
        [child.as_str()]
    );
    assert!(
        hub.blocked_by(&verify)
            .iter()
            .any(|(blocker, _)| blocker == child),
        "expected {verify} to be blocked by {child}, got {:?}",
        hub.blocked_by(&verify)
    );

    // Closing the reopened child unblocks the existing verify child without filing another.
    let reclosed = close(&mut hub, child);
    assert!(reclosed.filed.is_empty());
    assert_eq!(
        hub.list(json!({"ready": true, "ancestor": epic})),
        [verify.as_str()]
    );
}

#[test]
fn filing_a_new_child_makes_an_open_verify_child_wait_on_it() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create(
            "Epic",
            None,
            json!({"type": "epic", "acceptance": "Measured."})
        ),
        create("Child", Some("$0"), json!({})),
    ]));
    let [epic, child] = [&ids[0], &ids[1]];

    close(&mut hub, child);
    let verify = format!("{epic}.2");
    assert_eq!(
        hub.list(json!({"ready": true, "ancestor": epic})),
        [verify.as_str()]
    );

    let added = hub
        .ids(json!([create("Follow-up", Some(epic.as_str()), json!({}))]))
        .remove(0);
    assert_eq!(
        hub.list(json!({"ready": true, "ancestor": epic})),
        [added.as_str()]
    );
    assert!(
        hub.blocked_by(&verify)
            .iter()
            .any(|(blocker, _)| blocker == &added),
        "expected {verify} to be blocked by {added}, got {:?}",
        hub.blocked_by(&verify)
    );
}
