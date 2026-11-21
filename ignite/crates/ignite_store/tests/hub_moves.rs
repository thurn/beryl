//! Moving a ticket to another parent gives it and its subtree new hierarchical IDs, with the
//! old IDs kept as aliases.
// The shared helpers pass JSON by value, as the ported store tests do.
#![allow(clippy::needless_pass_by_value)]

mod common;

use serde_json::json;

use common::{Hub, create};
use ignite_store::StoreError;
use ignite_store::hub::renumber;

#[test]
fn a_moved_ticket_takes_its_new_parents_next_number_and_keeps_its_old_id_as_an_alias() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Old epic", None, json!({"type": "epic"})),
        create("New epic", None, json!({"type": "epic"})),
        create("Existing child", Some("$1"), json!({})),
        create("Moved", Some("$0"), json!({})),
        create("Below the moved one", Some("$3"), json!({})),
        create("Depends on the moved one", None, json!({})),
        {"op": "dep_add", "id": "$5", "on": "$3"},
        {"op": "dep_add", "id": "$4", "on": "$2", "kind": "related"},
        {"op": "comment", "id": "$3", "text": "Before the move"},
    ]));
    let [old_epic, new_epic, moved, below, dependent] =
        [&ids[0], &ids[1], &ids[3], &ids[4], &ids[5]];
    assert_eq!(*moved, format!("{old_epic}.1"));

    let renamed = hub
        .ids(json!([{"op": "update", "id": moved, "set": {"parent": new_epic}}]))
        .remove(0);
    assert_eq!(renamed, format!("{new_epic}.2"));
    let shown = hub.show(moved);
    assert_eq!(shown.ticket.id.as_str(), renamed);
    assert_eq!(
        shown
            .ticket
            .parent
            .as_ref()
            .map(ignite_core::ids::TicketId::as_str),
        Some(new_epic.as_str())
    );
    assert_eq!(
        shown
            .children
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!("{renamed}.1")]
    );
    assert_eq!(shown.comments.as_ref().unwrap()[0].text, "Before the move");
    let change = &shown.history.as_ref().unwrap().last().unwrap().change;
    assert_eq!(change["id"], json!([moved, renamed]));
    // The ticket below keeps its own number under the new prefix and its own edge.
    let below_now = hub.show(below);
    assert_eq!(below_now.ticket.id.as_str(), format!("{renamed}.1"));
    assert!(
        below_now
            .edges
            .iter()
            .any(|e| e.dst.as_str() == format!("{new_epic}.1"))
    );
    // A ticket that depended on the moved one now names its new ID.
    assert!(
        hub.show(dependent)
            .edges
            .iter()
            .any(|e| e.dst.as_str() == renamed)
    );
    assert!(hub.show(old_epic).children.is_empty());
    assert!(renumber::violations(&hub.conn).unwrap().is_empty());
    // The old ID still works in a later write.
    let commented = hub
        .ids(json!([{"op": "comment", "id": moved, "text": "After the move"}]))
        .remove(0);
    assert_eq!(commented, renamed);
}

#[test]
fn a_detached_ticket_gets_a_new_root_id_and_an_epic_keeps_its_e() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Epic", None, json!({"type": "epic"})),
        create("Task", Some("$0"), json!({})),
        create("Sub-epic", Some("$0"), json!({"type": "epic"})),
    ]));
    let task = hub
        .ids(json!([{"op": "update", "id": ids[1], "set": {"parent": null}}]))
        .remove(0);
    assert!(task.starts_with('T') && !task.contains('.'), "{task}");
    assert_eq!(hub.show(&task).ticket.parent, None);
    let epic = hub
        .ids(json!([{"op": "update", "id": ids[2], "set": {"parent": null}}]))
        .remove(0);
    assert!(epic.starts_with('E') && !epic.contains('.'), "{epic}");
    assert_eq!(hub.show(&ids[2]).ticket.id.as_str(), epic);
}

#[test]
fn a_ticket_with_a_claimed_ticket_below_it_cannot_move() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Epic", None, json!({})),
        create("Other epic", None, json!({})),
        create("Container", Some("$0"), json!({})),
        create("Claimed", Some("$2"), json!({})),
        {"op": "claim", "id": "$3"},
    ]));
    let refused = hub.post(json!([{"op": "update", "id": ids[2], "set": {"parent": ids[1]}}]));
    match refused {
        Err(StoreError::Rejected(message)) => {
            assert!(message.contains(&ids[3]), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(hub.show(&ids[2]).ticket.id.as_str(), ids[2]);
}

#[test]
fn naming_the_parent_a_ticket_already_has_changes_nothing() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Epic", None, json!({})),
        create("Task", Some("$0"), json!({})),
    ]));
    let same = hub
        .ids(json!([{"op": "update", "id": ids[1], "set": {"parent": ids[0]}}]))
        .remove(0);
    assert_eq!(same, ids[1]);
    assert_eq!(hub.show(&ids[0]).ticket.next_child, 2);
}

#[test]
fn moving_either_end_of_an_internal_dependency_outside_the_epic_is_refused() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Epic", None, json!({"type": "epic"})),
        create("Dependent", Some("$0"), json!({})),
        create("Prerequisite", Some("$0"), json!({})),
        create("Other epic", None, json!({"type": "epic"})),
        {"op": "dep_add", "id": "$1", "on": "$2"},
    ]));
    let [epic, dependent, prerequisite, other, ..] = &ids[..] else {
        panic!("unexpected IDs: {ids:?}")
    };
    for id in [dependent, prerequisite] {
        let result = hub.post(json!([{"op": "update", "id": id, "set": {"parent": other}}]));
        assert!(
            matches!(result, Err(StoreError::Rejected(message)) if message.contains("outside epic"))
        );
        assert_eq!(hub.show(id).ticket.id.as_str(), id);
    }
    assert_eq!(hub.show(epic).children.len(), 2);
    assert!(hub.show(other).children.is_empty());
    assert_eq!(hub.blocked_by(dependent), [common::pair(prerequisite, 2)]);
}
