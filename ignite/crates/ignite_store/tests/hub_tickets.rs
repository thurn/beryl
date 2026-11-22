//! The ticket service's store tests (`ticket-store.test.ts`), ported to the hub's database:
//! IDs and aliases, listing, readiness, closing ancestors and the graph.
// The tests pass JSON by value and compare against owned IDs, as the ported tests did.
#![allow(clippy::needless_pass_by_value, clippy::cloned_ref_to_slice_refs)]

mod common;

use serde_json::json;

use common::{Hub, create, create_with, pair};
use ignite_core::protocol::tickets::{EdgeKind, Status};
use ignite_store::StoreError;
use ignite_store::hub::{edges, tickets};

#[test]
fn an_epic_tree_starts_with_e_and_a_bare_root_finds_it() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Epic", None, json!({"type": "epic"})),
        create("Milestone", Some("$0"), json!({"type": "milestone"})),
        create("Task", None, json!({})),
    ]));
    let [epic, milestone, task] = [&ids[0], &ids[1], &ids[2]];
    // An epic's root starts with `E`, and its children take its ID whatever their type.
    assert!(epic.starts_with('E'), "{epic}");
    assert_eq!(*milestone, format!("{epic}.1"));
    assert!(task.starts_with('T'), "{task}");
    // A bare root finds the epic under its `E`, as it finds a ticket under its `T`.
    for (id, bare) in [
        (epic, &epic[1..]),
        (milestone, &milestone[1..]),
        (task, &task[1..]),
    ] {
        assert_eq!(hub.show(&bare.to_lowercase()).ticket.id.as_str(), id);
    }
}

#[test]
fn a_justiciar_root_and_its_children_resolve_with_or_without_the_j() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Emergency", None, json!({"labels": ["justiciar"]})),
        create("Follow-up", Some("$0"), json!({})),
    ]));
    let (root, child) = (&ids[0], &ids[1]);
    assert!(root.starts_with('J'), "{root}");
    assert_eq!(*child, format!("{root}.1"));
    for id in &ids {
        for name in [id.to_ascii_lowercase(), id[1..].to_ascii_lowercase()] {
            assert_eq!(hub.show(&name).ticket.id.as_str(), id);
        }
    }
    let bare_root = root[1..].to_ascii_lowercase();
    assert_eq!(hub.list(json!({"search": bare_root})), ids);
}

#[test]
fn creates_nested_ids_resolves_aliases_and_lists_using_every_filter() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create(
            "Epic",
            None,
            json!({"type": "epic", "body": "text", "design": "proposal"})
        ),
        create("Milestone", Some("$0"), json!({"type": "milestone"})),
        create_with(
            "Assigned bug",
            Some("$1"),
            json!({"type": "bug", "assignee": "dev", "labels": ["engine", "urgent"]}),
            json!({"deps": [{"on": "$0", "kind": "related"}]}),
        ),
        create("Closed task", Some("$0"), json!({"status": "closed"})),
    ]));
    let [epic, milestone, bug, archived] = [&ids[0], &ids[1], &ids[2], &ids[3]];
    let alphabet = "ABCDEFGHJKMNPQRSTVWXYZ";
    assert_eq!(epic.len(), 5);
    assert!(epic.starts_with('E'), "{epic}");
    assert!(alphabet.contains(&epic[1..2]), "{epic}");
    assert!(
        epic[1..]
            .chars()
            .all(|c| c.is_ascii_digit() || alphabet.contains(c))
    );
    assert_eq!(*milestone, format!("{epic}.1"));
    assert_eq!(*bug, format!("{milestone}.1"));
    assert_eq!(*archived, format!("{epic}.2"));

    hub.conn
        .execute(
            "INSERT INTO aliases VALUES ('old', ?1), ('lookup-legacy', ?2)",
            [epic, bug],
        )
        .unwrap();
    let shown = hub.show("old");
    assert_eq!(shown.ticket.id.as_str(), epic);
    let children: Vec<String> = shown.children.iter().map(ToString::to_string).collect();
    assert_eq!(children, [milestone.clone(), archived.clone()]);
    assert_eq!(shown.edges.len(), 1);
    assert_eq!(shown.edges[0].src.as_str(), bug);
    assert_eq!(hub.show("lookup-legacy").ticket.id.as_str(), bug);
    assert_eq!(hub.show(&format!("lookup-{bug}")).ticket.id.as_str(), bug);
    // The spellings from before IDs were stored in capitals still find the ticket.
    let old = format!("{}{}", &bug[..1], bug[1..].to_lowercase());
    assert_eq!(hub.show(&old).ticket.id.as_str(), bug);
    assert_eq!(hub.show(&old[1..]).ticket.id.as_str(), bug);
    assert_eq!(hub.show(&bug.to_lowercase()).ticket.id.as_str(), bug);
    assert_eq!(
        tickets::resolve(&hub.conn, "old")
            .unwrap()
            .unwrap()
            .as_str(),
        epic
    );
    assert!(tickets::resolve(&hub.conn, "missing").unwrap().is_none());

    let all3 = [epic.clone(), milestone.clone(), bug.clone()];
    assert_eq!(
        hub.list(json!({"parent": milestone.to_lowercase()})),
        [bug.clone()]
    );
    assert_eq!(hub.list(json!({})), all3);
    assert_eq!(
        hub.list(json!({"ancestor": "old"})),
        [milestone.clone(), bug.clone()]
    );
    assert_eq!(
        hub.list(json!({"all": true, "ancestor": "old"})),
        [milestone.clone(), bug.clone(), archived.clone()]
    );
    assert_eq!(hub.list(json!({"parent": "old"})), [milestone.clone()]);
    assert_eq!(hub.list(json!({"status": "closed"})), [archived.clone()]);
    assert_eq!(hub.list(json!({"all": true, "limit": 1})), [epic.clone()]);
    assert!(hub.list(json!({"limit": 0})).is_empty());
    assert!(hub.list(json!({"label": ["engine", "missing"]})).is_empty());
    let filtered = json!({"ready": true, "type": "bug", "status": "open",
        "label": ["engine", "urgent"], "assignee": "dev"});
    assert_eq!(hub.list(filtered), [bug.clone()]);
    assert_eq!(hub.list(json!({"label_any": ["urgent"]})), [bug.clone()]);
    assert_eq!(hub.list(json!({"search": "assigned"})), [bug.clone()]);
    assert_eq!(hub.list(json!({"search": "lookup-leg"})), [bug.clone()]);
    assert_eq!(hub.list(json!({"search": "closed"})), [archived.clone()]);

    let graph = tickets::graph(&hub.conn, false, hub.now).unwrap();
    let graphed: Vec<String> = graph
        .tickets
        .iter()
        .map(|t| t.ticket.id.to_string())
        .collect();
    assert_eq!(graphed, all3);
    assert!(graph.tickets[0].ticket.body.is_empty());
    assert!(graph.tickets[0].ticket.design.is_empty());
    assert_eq!(graph.edges, shown.edges);

    let version = hub.show(bug).ticket.version;
    let updated = hub.batch_if(
        json!([{"op": "update", "id": bug.to_lowercase(), "set": {"title": "Changed"}}]),
        json!([{"id": bug[1..].to_lowercase(), "version": version.0}]),
    );
    assert_eq!(updated.results[0].id.as_str(), bug);
    assert_eq!(hub.show(bug).ticket.title, "Changed");
    assert!(tickets::graph(&hub.conn, false, hub.now).unwrap().seq > graph.seq);
}

#[test]
fn reports_each_readiness_rule_across_ancestors_late_findings_and_parallel_audits() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Milestone", None, json!({"type": "milestone"})),
        create("Work", Some("$0"), json!({})),
        create("Audit A", Some("$0"), json!({"last": 1})),
        create("Audit B", Some("$0"), json!({"last": 1})),
        create_with("Release", None, json!({}), json!({"deps": [{"on": "$0", "kind": "waits-for"}]})),
        create("External blocker", None, json!({})),
        {"op": "dep_add", "id": "$0", "on": "$5", "kind": "conditional-blocks"},
        {"op": "dep_add", "id": "$1", "on": "$5"},
    ]));
    let [milestone, work, first, second, release, external] =
        [&ids[0], &ids[1], &ids[2], &ids[3], &ids[4], &ids[5]];
    assert_eq!(hub.blocked_by(work), [pair(external, 2), pair(external, 3)]);
    assert_eq!(hub.blocked_by(first), [pair(external, 3), pair(work, 4)]);
    assert!(hub.blocked_by(milestone).contains(&pair(work, 5)));
    assert!(hub.list(json!({})).contains(release));
    assert_eq!(hub.list(json!({"ready": true})), [external.clone()]);

    hub.batch(json!([
        {"op": "close", "id": external, "reason": "Fixed"},
        {"op": "close", "id": work, "reason": "Done"},
    ]));
    assert!(hub.show(first).ready);
    assert!(hub.show(second).ready);
    hub.batch(json!([{"op": "claim", "id": first}]));
    assert_eq!(hub.blocked_by(first), [pair(first, 1)]);
    assert!(hub.show(second).ready);
    let finding = hub
        .ids(json!([create("Late finding", Some(milestone), json!({}))]))
        .remove(0);
    assert_eq!(hub.blocked_by(second), [pair(&finding, 4)]);

    hub.batch(json!([
        {"op": "defer", "id": finding, "until": "2999-03-01T00:00:00Z"},
        {"op": "reopen", "id": finding},
    ]));
    assert_eq!(hub.blocked_by(&finding), [pair(&finding, 1)]);
    hub.batch(
        json!([{"op": "update", "id": finding, "set": {"defer_until": "2000-03-01T00:00:00Z"}}]),
    );
    assert!(hub.show(&finding).ready);
    hub.batch(json!([{"op": "close", "id": milestone, "reason": "Closed explicitly"}]));
    assert_eq!(hub.blocked_by(release), [pair(milestone, 2)]);
    let graph = tickets::graph(&hub.conn, false, hub.now).unwrap();
    assert!(!graph.edges.iter().any(|e| e.src.as_str() == release));
    let in_graph = |graph: &ignite_core::protocol::tickets::Graph, id: &str| {
        common::pairs(
            &graph
                .tickets
                .iter()
                .find(|t| t.ticket.id.as_str() == id)
                .unwrap()
                .blocked_by,
        )
    };
    assert_eq!(in_graph(&graph, release), [pair(milestone, 2)]);
    hub.batch(json!([{"op": "reopen", "id": external}]));
    let inherited = tickets::graph(&hub.conn, false, hub.now).unwrap();
    assert_eq!(
        in_graph(&inherited, second),
        [pair(external, 3), pair(&finding, 4)]
    );
    hub.batch(json!([{"op": "close", "id": external, "reason": "Fixed"}]));

    hub.batch(json!([
        {"op": "close", "id": finding, "reason": "Fixed"},
        {"op": "close", "id": first, "reason": "Done"},
    ]));
    assert!(hub.show(second).ready);
    hub.batch(json!([{"op": "close", "id": second, "reason": "Done"}]));
    assert!(hub.show(release).ready);
    let late = hub
        .ids(json!([create(
            "Another finding",
            Some(milestone),
            json!({})
        )]))
        .remove(0);
    assert_eq!(hub.blocked_by(release), [pair(milestone, 2)]);
    hub.batch(json!([{"op": "close", "id": late, "reason": "Fixed"}]));
    assert!(hub.show(release).ready);
}

#[test]
fn runs_milestone_audits_and_the_archivist_as_last_tickets() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Run", None, json!({"type": "epic"})),
        create("Milestone 1", Some("$0"), json!({"type": "epic"})),
        create_with(
            "Milestone 2",
            Some("$0"),
            json!({"type": "epic"}),
            json!({"deps": [{"on": "$1"}]})
        ),
        create("Build", Some("$1"), json!({})),
        create(
            "Mason",
            Some("$1"),
            json!({"last": 1, "labels": ["role:mason"]})
        ),
        create("Ship", Some("$2"), json!({})),
        create(
            "Archivist",
            Some("$0"),
            json!({"last": 1, "labels": ["role:archivist"]})
        ),
    ]));
    let [run, first, second, build, mason, ship, archivist] = [
        &ids[0], &ids[1], &ids[2], &ids[3], &ids[4], &ids[5], &ids[6],
    ];
    let ready = |hub: &Hub| hub.list(json!({"ready": true, "ancestor": run}));
    let close = |hub: &mut Hub, id: &str| {
        hub.batch(json!([{"op": "close", "id": id, "reason": "Done"}]))
            .results
            .remove(0)
    };
    let names = |ids: &[ignite_core::ids::TicketId]| -> Vec<String> {
        ids.iter().map(ToString::to_string).collect()
    };

    assert_eq!(ready(&hub), [build.clone()]);
    close(&mut hub, build);
    assert_eq!(ready(&hub), [mason.clone()]);
    hub.batch(json!([{"op": "claim", "id": mason}]));
    let finding = hub
        .ids(json!([create("Fix audit finding", Some(first), json!({}))]))
        .remove(0);
    close(&mut hub, mason);
    assert_eq!(ready(&hub), [finding.clone()]);
    assert_eq!(hub.show(first).ticket.status, Status::Open);

    assert_eq!(names(&close(&mut hub, &finding).closed), [first.clone()]);
    assert_eq!(ready(&hub), [ship.clone()]);
    assert_eq!(names(&close(&mut hub, ship).closed), [second.clone()]);
    assert_eq!(ready(&hub), [archivist.clone()]);
    assert_eq!(names(&close(&mut hub, archivist).closed), [run.clone()]);
    assert!(ready(&hub).is_empty());
}

#[test]
fn updates_fields_and_history_and_closes_every_now_empty_ancestor() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Epic", None, json!({})),
        create("Milestone", Some("$0"), json!({})),
        create("Task", Some("$1"), json!({})),
        create("Peer", None, json!({})),
    ]));
    let [epic, milestone, task, peer] = [&ids[0], &ids[1], &ids[2], &ids[3]];
    hub.batch(json!([
        {"op": "update", "id": task,
         "set": {"title": "Revised", "labels": ["old"], "metadata": {"discard": 1}},
         "add_labels": ["new"], "remove_labels": ["old"],
         "set_meta": {"kept": {"score": 3}}, "unset_meta": ["discard"]},
        {"op": "comment", "id": task, "text": "first"},
        {"op": "comment", "id": task, "text": "second"},
        {"op": "dep_add", "id": task, "on": peer},
    ]));
    assert!(!hub.show(task).ready);
    hub.batch(json!([{"op": "dep_remove", "id": task, "on": peer, "kind": "related"}]));
    assert!(!hub.show(task).ready);
    hub.batch(json!([{"op": "dep_remove", "id": task, "on": peer}]));
    let shown = hub.show(task);
    assert_eq!(shown.ticket.title, "Revised");
    assert_eq!(shown.ticket.labels, ["new"]);
    assert_eq!(json!(shown.ticket.metadata), json!({"kept": {"score": 3}}));
    assert!(shown.ready && shown.edges.is_empty());
    let comments = shown.comments.unwrap();
    let texts: Vec<(u32, &str, &str)> = comments
        .iter()
        .map(|c| (c.n, c.author.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(texts, [(1, "worker", "first"), (2, "worker", "second")]);
    assert_eq!(
        shown.history.unwrap()[1].change["title"],
        json!(["Task", "Revised"])
    );

    hub.batch(json!([{"op": "defer", "id": task, "until": "2999-03-01T00:00:00Z"}]));
    let deferred = hub.show(task);
    assert_eq!(
        (deferred.ticket.status, deferred.ready),
        (Status::Open, false)
    );
    hub.batch(json!([
        {"op": "reopen", "id": task},
        {"op": "update", "id": task, "set": {"defer_until": null}},
    ]));
    assert!(hub.show(task).ready);
    let reply = hub.batch(json!([
        {"op": "dep_add", "id": peer, "on": task},
        {"op": "close", "id": task, "reason": "Delivered", "include_ready": true},
        {"op": "claim", "id": peer},
    ]));
    let closed = &reply.results[1];
    let names: Vec<String> = closed.closed.iter().map(ToString::to_string).collect();
    assert_eq!(names, [milestone.clone(), epic.clone()]);
    let ready: Vec<String> = closed
        .ready
        .iter()
        .flatten()
        .map(ToString::to_string)
        .collect();
    assert_eq!(ready, [peer.clone()]);
    let shown = hub.show(epic).ticket;
    assert_eq!(shown.status, Status::Closed);
    assert_eq!(shown.close_reason.as_deref(), Some("Delivered"));
    hub.batch(json!([{"op": "reopen", "id": task}]));
    let reopened = hub.show(task).ticket;
    assert_eq!(reopened.status, Status::Open);
    assert!(reopened.closed_at.is_none() && reopened.close_reason.is_none());
}

#[test]
fn the_all_ticket_graph_keeps_closed_tickets_and_their_edges() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Live", None, json!({})),
        create_with(
            "Closed",
            None,
            json!({"status": "closed", "assignee": "old-worker", "notes": "long notes"}),
            json!({"deps": [{"on": "$0"}]}),
        ),
        {"op": "dep_add", "id": "$0", "on": "$1"},
    ]));
    let [live, archived] = [&ids[0], &ids[1]];
    let all = tickets::graph(&hub.conn, true, hub.now).unwrap();
    let saved = all
        .tickets
        .iter()
        .find(|t| t.ticket.id.as_str() == archived)
        .unwrap();
    assert_eq!(saved.ticket.assignee.as_deref(), Some("old-worker"));
    assert!(!saved.ready && saved.ticket.notes.is_empty());
    assert!(
        all.tickets
            .iter()
            .find(|t| t.ticket.id.as_str() == live)
            .unwrap()
            .ready
    );
    assert_eq!(all.edges.len(), 2);
    assert!(
        tickets::graph(&hub.conn, false, hub.now)
            .unwrap()
            .edges
            .is_empty()
    );

    hub.batch(json!([
        {"op": "update", "id": archived, "set": {"assignee": "new-worker"}},
        {"op": "dep_remove", "id": archived, "on": live},
    ]));
    let updated = tickets::graph(&hub.conn, true, hub.now).unwrap();
    let assignee = |graph: &ignite_core::protocol::tickets::Graph| {
        graph
            .tickets
            .iter()
            .find(|t| t.ticket.id.as_str() == archived)
            .unwrap()
            .ticket
            .assignee
            .clone()
    };
    assert_eq!(assignee(&updated).as_deref(), Some("new-worker"));
    assert_eq!(updated.edges.len(), 1);
    assert_eq!(updated.edges[0].src.as_str(), live);
    hub.batch(json!([{"op": "reopen", "id": archived}]));
    let reopened = tickets::graph(&hub.conn, true, hub.now).unwrap();
    assert_eq!(
        reopened
            .tickets
            .iter()
            .filter(|t| t.ticket.id.as_str() == archived)
            .count(),
        1
    );
    assert!(
        !reopened
            .tickets
            .iter()
            .find(|t| t.ticket.id.as_str() == live)
            .unwrap()
            .ready
    );
}

#[test]
fn a_defer_time_passing_makes_the_ticket_ready_at_the_same_sequence() {
    let mut hub = Hub::new();
    let id = hub.ids(json!([create("Later", None, json!({}))])).remove(0);
    let deadline = "2999-01-01T00:00:00.000Z";
    let seq = hub
        .batch(json!([{"op": "defer", "id": id, "until": deadline}]))
        .seq;
    hub.now = ignite_core::time::Timestamp::parse(deadline).unwrap();
    let before = tickets::graph(&hub.conn, false, hub.now).unwrap();
    assert_eq!(before.seq, seq);
    assert!(!before.tickets[0].ready);
    hub.now = hub.now.plus_seconds(0.001);
    let after = tickets::graph(&hub.conn, false, hub.now).unwrap();
    assert_eq!(after.seq, seq);
    assert!(after.tickets[0].ready && after.tickets[0].blocked_by.is_empty());
}

#[test]
fn reparenting_a_live_subtree_updates_readiness_and_rejects_ancestry_cycles() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Former parent", None, json!({})),
        create("Current parent", None, json!({})),
        create("Container", Some("$0"), json!({})),
        create("Task", Some("$2"), json!({})),
        create("External", None, json!({})),
        {"op": "dep_add", "id": "$1", "on": "$4"},
    ]));
    let [old, current, container, task, external] = [&ids[0], &ids[1], &ids[2], &ids[3], &ids[4]];
    assert!(hub.show(task).ready);
    let moved = hub
        .ids(json!([{"op": "update", "id": container, "set": {"parent": current}}]))
        .remove(0);
    // The container takes the new parent's first child number, and its task follows it.
    assert_eq!(moved, format!("{current}.1"));
    assert_eq!(
        hub.list(json!({"ancestor": current})),
        [moved.clone(), format!("{moved}.1")]
    );
    assert_eq!(hub.blocked_by(task), [pair(external, 3)]);
    assert!(hub.show(old).children.is_empty());
    let cycle = hub.post(json!([{"op": "update", "id": current, "set": {"parent": task}}]));
    assert!(
        matches!(cycle, Err(ignite_store::StoreError::Rejected(_))),
        "{cycle:?}"
    );
    assert!(hub.show(current).ticket.parent.is_none());
}

#[test]
fn serves_the_live_graph_listing_and_a_conditional_batch_with_5000_stored_tickets() {
    let mut hub = Hub::new();
    hub.conn
        .execute_batch(
            "WITH RECURSIVE ids(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM ids WHERE n < 5000)
             INSERT INTO tickets (id, parent, title, status, created_at, created_by, updated_at, version)
             SELECT printf('TSEED%04d', n), CASE WHEN n > 10 THEN printf('TSEED%04d', 1 + n % 10) END,
               printf('Ticket %04d', n), CASE WHEN n > 4700 THEN 'open' ELSE 'closed' END,
               '2026-11-01T00:00:00.000Z', 'seed', '2026-11-01T00:00:00.000Z', 0 FROM ids;
             INSERT INTO edges SELECT id, 'TSEED4999', 'blocks'
               FROM tickets WHERE status = 'open' AND id != 'TSEED4999';
             INSERT INTO edges SELECT id, 'TSEED0001', 'blocks'
               FROM tickets WHERE status = 'closed' AND parent IS NOT NULL;",
        )
        .unwrap();
    let graph = tickets::graph(&hub.conn, false, hub.now).unwrap();
    // The 300 open tickets; their closed parents are not live, so the graph leaves them out.
    assert_eq!(graph.tickets.len(), 300);
    assert_eq!(graph.edges.len(), 299);
    let all = tickets::graph(&hub.conn, true, hub.now).unwrap();
    assert_eq!(all.tickets.len(), 5000);
    assert_eq!(all.edges.len(), 4989);
    assert_eq!(edges::graph(&hub.conn, true).unwrap().len(), 4989);
    assert_eq!(hub.list(json!({})).len(), 300);
    assert_eq!(hub.list(json!({"ready": true})), ["TSEED4999"]);
    hub.batch_if(
        json!([{"op": "claim", "id": "TSEED4999"}]),
        json!([{"seq": graph.seq.0}, {"id": "TSEED4999", "assignee": null}]),
    );
    assert!(hub.list(json!({"ready": true})).is_empty());
}

#[test]
fn a_dependent_stays_blocked_until_the_last_child_of_its_prerequisite_closes() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Prerequisite", None, json!({})),
        create("Original child", Some("$0"), json!({})),
        create_with(
            "Dependent",
            None,
            json!({}),
            json!({"deps": [{"on": "$0"}]})
        ),
    ]));
    let [parent, original, dependent] = [&ids[0], &ids[1], &ids[2]];
    let child = hub
        .ids(json!([create("New child", Some(parent), json!({}))]))
        .remove(0);
    hub.batch(json!([{"op": "close", "id": original, "reason": "Complete"}]));
    assert!(
        hub.show(parent)
            .children
            .iter()
            .any(|c| c.as_str() == child)
    );
    assert!(!hub.show(dependent).ready);
    hub.batch(json!([{"op": "close", "id": child, "reason": "Complete"}]));
    assert_eq!(hub.show(parent).ticket.status, Status::Closed);
    assert!(hub.show(dependent).ready);
}

#[test]
fn a_root_id_with_a_dash_from_an_earlier_tracker_is_read_found_and_listed() {
    // Thousands of the ticket service's closed tickets have IDs such as `TCI-00031CED38`
    // (`ci-00031ced38` before IDs were stored in capitals), with an alias carrying the project
    // prefix in lowercase.
    let mut hub = Hub::new();
    hub.conn
        .execute_batch(
            "INSERT INTO tickets (id, title, status, created_at, created_by, updated_at, version)
               VALUES ('TCI-0003', 'CI run', 'closed', '2026-11-01T00:00:00Z', 'tk',
                       '2026-11-01T00:00:00Z', 1);
             INSERT INTO aliases VALUES ('pj-ci-0003', 'TCI-0003');",
        )
        .unwrap();
    for name in [
        "TCI-0003",
        "ci-0003",
        "Tci-0003",
        "pj-ci-0003",
        "PJ-CI-0003",
    ] {
        assert_eq!(hub.show(name).ticket.id.as_str(), "TCI-0003", "{name}");
    }
    assert_eq!(hub.list(json!({"all": true})), ["TCI-0003"]);
    let id = hub
        .ids(json!([{"op": "comment", "id": "ci-0003", "text": "still here"}]))
        .remove(0);
    assert_eq!(id, "TCI-0003");
}

#[test]
fn a_null_parent_detaches_a_ticket() {
    let mut hub = Hub::new();
    let epic = hub.ids(json!([create("Epic", None, json!({}))])).remove(0);
    let child = hub
        .ids(json!([create("Child", Some(&epic), json!({}))]))
        .remove(0);
    hub.batch(json!([{"op": "update", "id": child, "set": {"parent": null}}]));
    assert_eq!(hub.show(&child).ticket.parent, None);
    assert!(hub.show(&epic).children.is_empty());
}

#[test]
fn an_epic_filed_without_workers_gets_the_default_share_and_an_explicit_one_is_kept() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Default", None, json!({"type": "epic"})),
        create("Parked", None, json!({"type": "epic", "workers": 0})),
        create("Wide", None, json!({"type": "epic", "workers": 6})),
        create("Task", None, json!({})),
    ]));
    let workers: Vec<Option<u32>> = ids.iter().map(|id| hub.show(id).ticket.workers).collect();
    assert_eq!(
        workers,
        [
            Some(ignite_core::shares::DEFAULT_EPIC_ALLOCATION),
            Some(0),
            Some(6),
            None
        ]
    );
}

/// Applies `ops` through the hub's filer-dependency step, as `actor` does, and returns the IDs
/// created and what the step added.
fn file_as(
    hub: &mut Hub,
    actor: &str,
    ops: serde_json::Value,
) -> (
    Vec<String>,
    Vec<ignite_store::hub::filer_deps::AddedDependency>,
) {
    let mut request: ignite_core::protocol::tickets::BatchRequest =
        serde_json::from_value(json!({"ops": ops})).unwrap();
    let added =
        ignite_store::hub::filer_deps::add_filer_dependencies(&hub.conn, &mut request, actor)
            .unwrap();
    let ops = serde_json::to_value(&request.ops).unwrap();
    (hub.ids_as(actor, ops), added)
}

#[test]
fn a_ticket_filed_beside_the_filers_unlanded_ticket_depends_on_it() {
    let mut hub = Hub::new();
    let epic = hub.ids(json!([create("Epic", None, json!({"type": "epic"}))]))[0].clone();
    let ids = hub.ids(json!([
        create("Held", Some(&epic), json!({})),
        create("Other", Some(&epic), json!({})),
        create("Elsewhere", None, json!({})),
    ]));
    hub.post_as("me", json!([{"op": "claim", "id": ids[0]}]), json!([]))
        .unwrap();

    // A sibling of the held ticket depends on it; a child (which the held ticket waits for), a
    // ticket in another line, one that already lists it, and another session's filing do not.
    let (created, added) = file_as(
        &mut hub,
        "me",
        json!([
            create("Sibling", Some(&epic), json!({})),
            create("Child", Some(&ids[0]), json!({})),
            create("Unrelated", None, json!({})),
            create_with(
                "Listed",
                Some(&epic),
                json!({}),
                json!({"deps": [{"on": ids[0]}]})
            ),
        ]),
    );
    assert_eq!(
        added
            .iter()
            .map(|a| (a.op, a.on.to_string()))
            .collect::<Vec<_>>(),
        [(0, ids[0].clone())]
    );
    assert_eq!(hub.blocked_by(&created[0]), [pair(&ids[0], 2)]);
    assert_eq!(hub.blocked_by(&created[1]), []);
    assert_eq!(hub.blocked_by(&created[2]), []);
    let (_, none) = file_as(
        &mut hub,
        "someone-else",
        json!([create("Mine", Some(&epic), json!({}))]),
    );
    assert!(none.is_empty());
}

#[test]
fn an_epic_cannot_gain_an_external_prerequisite_during_filing_or_after_release() {
    let mut hub = Hub::new();
    let external = hub.ids(json!([create("External", None, json!({}))]))[0].clone();
    let filed = hub.post(json!([
        create(
            "Epic",
            None,
            json!({"type": "epic", "status": "pending_approval"})
        ),
        create_with(
            "Dependent",
            Some("$0"),
            json!({}),
            json!({"deps": [{"on": external}]}),
        ),
    ]));
    assert!(matches!(filed, Err(StoreError::Rejected(message)) if message.contains(&external)));
    assert_eq!(hub.list(json!({})), [external.clone()]);

    let ids = hub.ids(json!([
        create("Epic", None, json!({"type": "epic", "status": "open"})),
        create("Dependent", Some("$0"), json!({})),
        {"op": "dep_add", "id": "$1", "on": external, "kind": "related"},
    ]));
    let dependent = &ids[1];
    let edited = hub.post(json!([
        {"op": "update", "id": dependent, "set": {"title": "Changed"}},
        {"op": "dep_add", "id": dependent, "on": external, "kind": "waits-for"},
    ]));
    assert!(matches!(edited, Err(StoreError::Rejected(message)) if message.contains(&ids[0])));
    let shown = hub.show(dependent);
    assert_eq!(shown.ticket.title, "Dependent");
    assert_eq!(shown.edges[0].kind, EdgeKind::Related);
    assert!(shown.ready);
}

#[test]
fn a_pending_epic_with_a_legacy_external_dependency_cannot_be_released() {
    let mut hub = Hub::new();
    let ids = hub.ids(json!([
        create("Outside", None, json!({})),
        create(
            "Epic",
            None,
            json!({"type": "epic", "status": "pending_approval"})
        ),
        create(
            "Dependent",
            Some("$1"),
            json!({"status": "pending_approval"})
        ),
    ]));
    let (outside, epic, dependent) = (&ids[0], &ids[1], &ids[2]);
    edges::add(&hub.conn, dependent, outside, EdgeKind::Blocks).unwrap();
    let release = json!({"op": "update", "id": epic, "set": {"status": "open"}});
    let result = hub.post(json!([release]));
    assert!(matches!(result, Err(StoreError::Rejected(message)) if message.contains(dependent)));
    assert_eq!(hub.show(epic).ticket.status, Status::PendingApproval);
    hub.batch(json!([
        {"op": "dep_remove", "id": dependent, "on": outside},
        release,
    ]));
    assert_eq!(hub.show(epic).ticket.status, Status::Open);
}
