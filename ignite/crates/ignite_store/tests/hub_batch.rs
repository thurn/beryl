//! The ticket service's batch tests (`ticket-store.test.ts`), ported: preconditions, racing
//! claims, rollback, request keys and the feed a batch leaves.
// The tests pass JSON by value and compare against owned IDs, as the ported tests did.
#![allow(clippy::needless_pass_by_value, clippy::cloned_ref_to_slice_refs)]

mod common;

use std::sync::{Arc, Barrier};

use serde_json::json;

use common::{Hub, create, create_with};
use ignite_core::ids::Seq;
use ignite_core::protocol::tickets::{BatchRequest, Status};
use ignite_store::StoreError;
use ignite_store::hub::{self, batch, feed, tickets};

#[test]
fn checks_each_precondition_and_reports_the_one_that_failed() {
    let mut hub = Hub::new();
    let reply = hub.batch(json!([create("Claimable", None, json!({}))]));
    let (seq, id) = (reply.seq.0, reply.results[0].id.to_string());
    let version = hub.show(&id).ticket.version.0;
    let valid = [
        json!({"seq": seq}),
        json!({"id": id, "version": version}),
        json!({"id": id, "assignee": null}),
        json!({"id": id, "status": "open"}),
    ];
    let invalid = [
        json!({"seq": seq + 1}),
        json!({"id": id, "version": seq + 1}),
        json!({"id": id, "assignee": "other"}),
        json!({"id": id, "status": "blocked"}),
    ];
    for (index, wrong) in invalid.into_iter().enumerate() {
        let mut conditions = valid.to_vec();
        conditions[index] = wrong;
        let conflict = hub.conflict(json!([{"op": "claim", "id": id}]), json!(conditions));
        assert_eq!(conflict.failed, index);
        match &conflict.current {
            None => assert_eq!(index, 0, "only a seq precondition has no ticket"),
            Some(ticket) => assert_eq!(
                (ticket.id.as_str(), ticket.status),
                (id.as_str(), Status::Open)
            ),
        }
    }
    assert_eq!(
        hub.show(&id).ticket.status,
        Status::Open,
        "a failed batch changes nothing"
    );
    hub.batch_if(json!([{"op": "claim", "id": id}]), json!(valid));
    let lost = hub
        .post_as("other", json!([{"op": "claim", "id": id}]), json!([]))
        .unwrap();
    let batch::BatchOutcome::Conflict(lost) = lost else {
        panic!("a claim of a held ticket must conflict")
    };
    assert_eq!(lost.failed, 0);
    assert_eq!(lost.current.unwrap().assignee.as_deref(), Some("worker"));
}

#[test]
fn of_two_simultaneous_claims_one_wins_and_one_conflicts() {
    let dir =
        ignite_core::test_files::temp_dir().join(format!("ignite-claims-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hub.sqlite");
    let _ = std::fs::remove_file(&path);
    let mut setup = Hub {
        conn: hub::open(&path).unwrap(),
        now: Hub::new().now,
    };
    let id = setup
        .ids(json!([create("Claimable", None, json!({}))]))
        .remove(0);
    let request: BatchRequest = serde_json::from_value(json!({
        "if": [{"id": id, "assignee": null}],
        "ops": [{"op": "claim", "id": id}],
    }))
    .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|actor| {
            let (path, request, barrier, now) = (
                path.clone(),
                request.clone(),
                Arc::clone(&barrier),
                setup.now,
            );
            std::thread::spawn(move || {
                let mut conn = hub::open(&path).unwrap();
                barrier.wait();
                let outcome = batch::apply(&mut conn, &request, actor, "m-test", now).unwrap();
                (actor, matches!(outcome, batch::BatchOutcome::Applied(_)))
            })
        })
        .collect();
    let outcomes: Vec<(&str, bool)> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let winners: Vec<&str> = outcomes
        .iter()
        .filter(|(_, won)| *won)
        .map(|(a, _)| *a)
        .collect();
    assert_eq!(winners.len(), 1, "{outcomes:?}");
    let shown = setup.show(&id).ticket;
    assert_eq!(shown.assignee.as_deref(), Some(winners[0]));
    assert_eq!(shown.status, Status::InProgress);
    assert_eq!(
        shown.machine.as_ref().map(ToString::to_string).as_deref(),
        Some("m-test")
    );
    assert_eq!(shown.claimed_at, Some(setup.now));
    drop(setup);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_batch_rolls_back_every_row_and_request_keys_deduplicate_creates() {
    let mut hub = Hub::new();
    let reply = hub.batch(json!([
        create("Parent", None, json!({})),
        create("Already claimed", None, json!({"assignee": "other"})),
    ]));
    let (seq, parent, claimed) = (
        reply.seq,
        reply.results[0].id.to_string(),
        reply.results[1].id.to_string(),
    );
    let before = tickets::graph(&hub.conn, true, hub.now).unwrap();
    let conflict = hub.conflict(
        json!([
            create_with("Child", Some(&parent), json!({}), json!({"request_key": "child-1"})),
            {"op": "comment", "id": parent, "text": "Rollback"},
            {"op": "dep_add", "id": parent, "on": claimed},
            {"op": "claim", "id": claimed},
        ]),
        json!([]),
    );
    assert_eq!(conflict.failed, 3);
    assert_eq!(conflict.current.unwrap().id.as_str(), claimed);
    assert_eq!(tickets::graph(&hub.conn, true, hub.now).unwrap(), before);
    let shown = hub.show(&parent);
    assert_eq!(shown.ticket.next_child, 1);
    assert!(shown.children.is_empty() && shown.edges.is_empty());
    assert!(shown.comments.unwrap().is_empty());
    assert_eq!(feed::current(&hub.conn).unwrap(), seq);

    let missing = hub.post(json!([
        create("Child", Some(&parent), json!({})),
        {"op": "comment", "id": "missing", "text": ""},
    ]));
    assert!(
        matches!(missing, Err(StoreError::NotFound(_))),
        "{missing:?}"
    );
    let reference = hub.post(json!([
        create("Child", Some(&parent), json!({})),
        {"op": "update", "id": "$8", "set": {"title": "Bad"}},
    ]));
    assert!(
        matches!(reference, Err(StoreError::Rejected(_))),
        "{reference:?}"
    );
    let untitled = hub.post(json!([{"op": "create", "fields": {}}]));
    assert!(
        matches!(untitled, Err(StoreError::Rejected(_))),
        "{untitled:?}"
    );
    assert_eq!(hub.show(&parent).ticket.next_child, 1);

    let op = create_with(
        "Child",
        Some(&parent),
        json!({}),
        json!({"request_key": "child-1"}),
    );
    let accepted = hub.batch_if(json!([op.clone()]), json!([{"seq": seq.0}]));
    assert_eq!(accepted.results[0].id.to_string(), format!("{parent}.1"));
    let retried = hub.batch(json!([op.clone()]));
    assert_eq!(retried.results, accepted.results);
    assert_eq!(retried.seq, accepted.seq, "a retried create writes nothing");
    let children: Vec<String> = hub
        .show(&parent)
        .children
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(children, [format!("{parent}.1")]);
    hub.now = hub.now.plus_seconds(86_401.0);
    let later = hub.batch(json!([op]));
    assert_eq!(
        later.results[0].id.to_string(),
        format!("{parent}.2"),
        "a key lasts one day"
    );
}

#[test]
fn versions_change_with_fields_while_comments_and_edges_only_move_the_feed() {
    let mut hub = Hub::new();
    let reply = hub.batch(json!([
        create("First", None, json!({})),
        create("Second", None, json!({}))
    ]));
    let (seq, first, second) = (
        reply.seq,
        reply.results[0].id.to_string(),
        reply.results[1].id.to_string(),
    );
    let version = hub.show(&first).ticket.version;
    let changed = hub.batch(json!([
        {"op": "comment", "id": first, "text": "An update"},
        {"op": "dep_add", "id": first, "on": second},
    ]));
    assert_eq!(hub.show(&first).ticket.version, version);
    assert_eq!(changed.seq, Seq(seq.0 + 2));
    let batch = feed::since(&hub.conn, seq, 100).unwrap();
    assert_eq!(batch.seq, changed.seq);
    let mut touched: Vec<String> = batch
        .changes
        .iter()
        .map(|c| c.ticket.as_ref().unwrap().to_string())
        .collect();
    touched.sort();
    let mut expected = vec![first.clone(), second.clone()];
    expected.sort();
    assert_eq!(touched, expected);
    assert!(
        batch
            .changes
            .iter()
            .all(|c| c.kind == "ticket" && c.seq == changed.seq)
    );

    let unchanged = hub.batch(json!([{"op": "update", "id": first, "set": {"title": "First"}}]));
    assert_eq!(
        unchanged.seq, changed.seq,
        "an update that changes nothing takes no position"
    );
    let history = hub.show(&first).history.unwrap();
    assert_eq!(history.len(), 1, "only the create is in the history");
}

#[test]
fn a_closed_create_and_a_claim_by_the_holder_behave_as_before() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Done already", None, json!({"status": "closed"})),
        create("Mine", None, json!({})),
    ]));
    assert_eq!(hub.show(&ids[0]).ticket.closed_at, Some(hub.now));
    hub.batch(json!([{"op": "claim", "id": ids[1]}]));
    let version = hub.show(&ids[1]).ticket.version;
    hub.now = hub.now.plus_seconds(60.0);
    hub.batch(json!([{"op": "claim", "id": ids[1]}]));
    let again = hub.show(&ids[1]).ticket;
    assert_eq!(
        again.version, version,
        "claiming a ticket you hold changes nothing"
    );
    assert_eq!(again.claimed_at, Some(hub.now.plus_seconds(-60.0)));
}

#[test]
fn a_create_stores_a_saving_and_refuses_one_with_no_known_unit_or_amount() {
    let mut hub = Hub::new();
    let saving = |unit: &str, amount: f64| {
        json!({"saving": {"rule": "estimate", "subject": "the gate", "unit": unit,
                          "amount": amount}})
    };
    let id = hub
        .ids(json!([create(
            "Shorten the gate's wait",
            None,
            saving("agent-hours", 12.5)
        )]))
        .remove(0);
    let stored = tickets::load(&hub.conn, &id)
        .unwrap()
        .unwrap()
        .saving
        .unwrap();
    assert_eq!((stored.unit.as_str(), stored.amount), ("agent-hours", 12.5));
    for (unit, amount) in [("minutes", 12.0), ("agent-hours", 0.0), ("dollars", -5.0)] {
        let refused = hub.post(json!([create("Refused", None, saving(unit, amount))]));
        assert!(
            matches!(refused, Err(StoreError::Rejected(_))),
            "{unit} {amount}: {refused:?}"
        );
    }
}

#[test]
fn closing_a_ticket_through_a_batch_closes_its_finding() {
    let mut hub = Hub::new();
    let reply = hub.batch(json!([
        create("Improve", None, json!({})),
        create("Other", None, json!({}))
    ]));
    let (improve, other) = (
        reply.results[0].id.to_string(),
        reply.results[1].id.to_string(),
    );
    for ticket in [&improve, &other] {
        hub::findings::upsert(
            &hub.conn,
            &hub::rows::Finding {
                rule: "slow-verb".to_owned(),
                subject: format!("subject-{ticket}"),
                unit: "seconds".to_owned(),
                cost: 10.0,
                saving: 5.0,
                ticket: Some(ticket.clone()),
                closed_at: None,
            },
        )
        .unwrap();
    }
    hub.batch(json!([{"op": "close", "id": improve, "reason": "done"}]));
    let closed: Vec<(String, bool)> = hub
        .conn
        .prepare("SELECT ticket, closed_at IS NOT NULL FROM findings ORDER BY subject")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(closed.contains(&(improve, true)));
    assert!(closed.contains(&(other, false)));
}
