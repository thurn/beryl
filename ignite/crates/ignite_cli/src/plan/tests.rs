//! The plan check against seeded graphs, with no hub.

use std::collections::{BTreeSet, HashSet};

use ignite_core::protocol::tickets::{Edge, Ticket};
use serde_json::json;

use super::Graphs;

const EPIC: &str = "ig9a";

fn ticket(id: &str, parent: &str, body: &str, labels: &[&str]) -> Ticket {
    serde_json::from_value(json!({
        "id": id, "parent": (!parent.is_empty()).then_some(parent), "next_child": 1, "title": id, "body": body,
        "type": "task", "status": "open", "priority": 2, "assignee": null,
        "labels": labels, "defer_until": null, "close_reason": null,
        "created_at": "2026-11-18T00:00:00.000Z", "created_by": "t",
        "updated_at": "2026-11-18T00:00:00.000Z", "closed_at": null, "version": 1,
    }))
    .unwrap()
}

fn edge(src: &str, dst: &str) -> Edge {
    serde_json::from_value(json!({"src": src, "dst": dst})).unwrap()
}

fn id(n: u32) -> String {
    format!("{EPIC}.{n}")
}

fn report(tickets: &[Ticket], edges: &[Edge], ready: &[String]) -> Vec<String> {
    let ready: HashSet<String> = ready.iter().cloned().collect();
    let machines: BTreeSet<String> = ["jarvis"].map(str::to_owned).into();
    Graphs {
        epic: EPIC,
        tickets,
        everything: tickets,
        edges,
        ready: &ready,
        machines: &machines,
        providers: &["claude".to_owned()].into(),
        repository: None,
    }
    .report()
}

#[test]
fn a_seeded_epic_reproduces_the_documents_example_line_for_line() {
    let tickets: Vec<Ticket> = (1..=41)
        .map(|n| {
            let body = match n {
                19 | 27 => "Nothing to read.".to_owned(),
                7 => "Edits `crates/seven.rs`.".to_owned(),
                12 => "Edits `crates/twelve.rs`.".to_owned(),
                _ => "Edits `crates/shared.rs`.".to_owned(),
            };
            let labels: &[&str] = if n == 31 || n == 33 {
                &["lane:replay"]
            } else {
                &[]
            };
            ticket(&id(n), EPIC, &body, labels)
        })
        .collect();
    let mut edges: Vec<Edge> = [(4, 1), (9, 4), (15, 9), (22, 15), (30, 22)]
        .iter()
        .map(|&(a, b)| edge(&id(a), &id(b)))
        .collect();
    edges.push(edge(&id(12), &id(7)));
    let ready: Vec<String> = (1..=41)
        .filter(|n| ![4, 9, 15, 22, 30, 12].contains(n))
        .take(17)
        .map(id)
        .collect();
    assert_eq!(
        report(&tickets, &edges, &ready),
        [
            "tickets        41, ready at the start 17",
            "longest chain  6: ig9a.1 > .4 > .9 > .15 > .22 > .30",
            "suspect edge   .12 depends on .7, and they name no file in common",
            "no files       .19, .27 name no files to read",
            "lane           lane:replay holds .31 and .33, which do not depend on each other",
        ]
        .map(|line| line.replace("ig9a.1 >", "Tig9a.1 >"))
    );
}

#[test]
fn a_clean_epic_reports_only_its_shape() {
    let tickets = [
        ticket(&id(1), EPIC, "`a/x.rs`", &["lane:one"]),
        ticket(&id(2), EPIC, "`a/x.rs`", &["lane:one"]),
    ];
    let edges = [edge(&id(2), &id(1))];
    assert_eq!(
        report(&tickets, &edges, &[id(1)]),
        [
            "tickets        2, ready at the start 1",
            "longest chain  2: Tig9a.1 > .2"
        ]
    );
}

#[test]
fn unknown_machine_and_provider_pins_are_reported() {
    let tickets = [
        ticket(
            &id(1),
            EPIC,
            "`a.rs`",
            &["machine:cygnus", "provider:codex"],
        ),
        ticket(
            &id(2),
            EPIC,
            "`a.rs`",
            &["machine:jarvis", "provider:claude"],
        ),
        ticket(&id(3), EPIC, "`a.rs`", &["needs:person"]),
    ];
    let lines = report(&tickets, &[], &[]);
    assert_eq!(
        lines[2..],
        [
            "no machine     .1 is pinned to cygnus, which the fleet does not know",
            "no provider    .1 is pinned to codex, which the fleet does not know"
        ]
    );
}

#[test]
fn a_dependency_on_an_epic_a_report_with_dependents_and_a_moved_ticket_each_get_a_line() {
    let mut sub = ticket(&id(1), EPIC, "`a.rs`", &[]);
    sub.next_child = 2;
    let child = ticket("ig9a.1.1", &id(1), "`a.rs`", &[]);
    let measure = ticket(&id(2), EPIC, "`a.rs`", &["report"]);
    let user = ticket(&id(3), EPIC, "`a.rs`", &[]);
    let waiter = ticket(&id(4), EPIC, "`a.rs`", &[]);
    // Created under another epic, then moved here.
    let moved = ticket("other.5", EPIC, "`a.rs`", &[]);
    let other = ticket("other", "", "", &[]);
    let tickets = [sub, child, measure, user, waiter, moved];
    let mut everything = tickets.to_vec();
    everything.push(other);
    let edges = [
        edge(&id(4), &id(1)),
        edge(&id(3), &id(2)),
        edge("ig9a.1.1", &id(2)),
    ];
    let ready = HashSet::new();
    let machines = BTreeSet::new();
    let lines = Graphs {
        epic: EPIC,
        tickets: &tickets,
        everything: &everything,
        edges: &edges,
        ready: &ready,
        machines: &machines,
        providers: &["claude".to_owned()].into(),
        repository: None,
    }
    .report();
    assert_eq!(
        lines[2..],
        [
            "epic edge      .4 depends on the epic .1; depend on the ticket that needs its result",
            "report         .2 is labeled report, and .3 and .1.1 depend on it",
            "moved in       Tother.5 was moved in from the open epic Tother",
        ]
    );
}

#[test]
fn a_files_line_names_files_short_paths_resolve_and_missing_paths_are_reported() {
    let repository = super::Repository {
        root: ignite_core::test_files::temp_dir().join("ignite-plan-no-such-checkout"),
        tracked: ["rust/client/crates/game_scene/src/style.rs", "tools/b.py"]
            .map(str::to_owned)
            .to_vec(),
    };
    // .1 names its files without backquotes, one by a short path; .2 names the full path and
    // depends on .1, so the two share a file once the short path resolves.
    let tickets = [
        ticket(
            &id(1),
            EPIC,
            "Files: game_scene/src/style.rs, tools/b.py",
            &[],
        ),
        ticket(
            &id(2),
            EPIC,
            "Edit `rust/client/crates/game_scene/src/style.rs` and `tools/gone.py`, \
             not `ticket.body`, and write `$CACHE/x.json`, `data/<target>.json` and `game_scene/src/...`.",
            &[],
        ),
    ];
    let edges = [edge(&id(2), &id(1))];
    let ready: HashSet<String> = [id(1)].into_iter().collect();
    let machines = BTreeSet::new();
    let lines = Graphs {
        epic: EPIC,
        tickets: &tickets,
        everything: &tickets,
        edges: &edges,
        ready: &ready,
        machines: &machines,
        providers: &["claude".to_owned()].into(),
        repository: Some(&repository),
    }
    .report();
    assert_eq!(
        lines[2..],
        ["missing file   .2 names tools/gone.py, which does not exist"]
    );
}
