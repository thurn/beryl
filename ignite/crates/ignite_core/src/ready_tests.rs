use super::*;
use crate::ids::Seq;
use crate::protocol::tickets::{EdgeKind, TicketType};

fn id(text: &str) -> TicketId {
    text.parse().unwrap()
}

fn ticket(text: &str) -> Ticket {
    let id = id(text);
    Ticket {
        parent: id.parent(),
        id,
        next_child: 1,
        title: text.to_owned(),
        body: String::new(),
        acceptance: String::new(),
        design: String::new(),
        notes: String::new(),
        ticket_type: TicketType::Task,
        status: Status::Open,
        priority: 2,
        assignee: None,
        last: 0,
        labels: Vec::new(),
        metadata: BTreeMap::new(),
        defer_until: None,
        close_reason: None,
        created_at: Timestamp::EPOCH,
        created_by: "test".to_owned(),
        updated_at: Timestamp::EPOCH,
        closed_at: None,
        version: Seq(0),
        workers: None,
        allocation: None,
        max_workers: None,
        estimate_minutes: None,
        unfiled_minutes: None,
        cores: None,
        machine: None,
        session: None,
        tree: None,
        claimed_at: None,
        landed_at: None,
        landed: None,
        saving: None,
        needs_os: None,
        fallback: false,
        every_hours: None,
    }
}

fn closed(text: &str) -> Ticket {
    Ticket {
        status: Status::Closed,
        ..ticket(text)
    }
}

fn last(text: &str) -> Ticket {
    Ticket {
        last: 1,
        ..ticket(text)
    }
}

fn edge(src: &str, dst: &str, kind: EdgeKind) -> Edge {
    Edge {
        src: id(src),
        dst: id(dst),
        kind,
    }
}

fn blocker(text: &str, rule: u8) -> Blocker {
    Blocker { id: id(text), rule }
}

const NOW: Timestamp = Timestamp::from_millis(1_000_000);

fn of(tickets: &[Ticket], edges: &[Edge], which: &str) -> Vec<Blocker> {
    blockers_by_id(tickets, edges, NOW)
        .remove(&id(which))
        .unwrap()
}

#[test]
fn reports_each_rule_with_its_ticket() {
    // A milestone `m` with work `m.1` and two audits `m.2` and `m.3` that run last, a release
    // `r` that waits for the milestone, and an external blocker `x` of both the milestone and
    // the work.
    let tickets = [
        ticket("m"),
        ticket("m.1"),
        last("m.2"),
        last("m.3"),
        ticket("r"),
        ticket("x"),
    ];
    let edges = [
        edge("r", "m", EdgeKind::WaitsFor),
        edge("m", "x", EdgeKind::ConditionalBlocks),
        edge("m.1", "x", EdgeKind::Blocks),
    ];
    assert_eq!(
        of(&tickets, &edges, "m.1"),
        [blocker("x", 2), blocker("x", 3)]
    );
    assert_eq!(
        of(&tickets, &edges, "m.2"),
        [blocker("x", 3), blocker("m.1", 4)]
    );
    assert!(of(&tickets, &edges, "m").contains(&blocker("m.1", 5)));
    assert_eq!(ready(&tickets, &edges, NOW), [id("x")]);
}

#[test]
fn a_claimed_or_deferred_ticket_blocks_itself_until_its_time_passes() {
    let claimed = Ticket {
        status: Status::InProgress,
        ..ticket("a")
    };
    let deferred = Ticket {
        defer_until: Some(NOW),
        ..ticket("b")
    };
    let passed = Ticket {
        defer_until: Some(NOW.plus_seconds(-1.0)),
        ..ticket("c")
    };
    let tickets = [claimed, deferred, passed];
    assert_eq!(of(&tickets, &[], "a"), [blocker("a", 1)]);
    assert_eq!(of(&tickets, &[], "b"), [blocker("b", 1)]);
    assert_eq!(ready(&tickets, &[], NOW), [id("c")]);
}

#[test]
fn a_closed_dependency_with_a_live_child_still_blocks() {
    // `m` was closed by hand while its child `m.1` is open, so `r`, which depends on `m`, waits.
    let tickets = [closed("m"), ticket("m.1"), ticket("r")];
    let edges = [edge("r", "m", EdgeKind::Blocks)];
    assert_eq!(of(&tickets, &edges, "r"), [blocker("m", 2)]);
    let finished = [closed("m"), closed("m.1"), ticket("r")];
    assert_eq!(ready(&finished, &edges, NOW), [id("r")]);
}

#[test]
fn a_closed_ancestor_passes_its_blockers_down() {
    let tickets = [closed("e"), closed("e.1"), ticket("e.1.1"), ticket("x")];
    let edges = [edge("e", "x", EdgeKind::Blocks)];
    assert_eq!(of(&tickets, &edges, "e.1.1"), [blocker("x", 3)]);
}

#[test]
fn related_and_discovered_edges_never_block() {
    let tickets = [ticket("a"), ticket("b"), ticket("c")];
    let edges = [
        edge("a", "b", EdgeKind::Related),
        edge("a", "c", EdgeKind::DiscoveredFrom),
    ];
    assert!(of(&tickets, &edges, "a").is_empty());
}

#[test]
fn last_tickets_wait_only_for_siblings_that_are_not_last() {
    // A finding filed under the milestone after the audits holds both audits back, and the
    // audits never hold each other back.
    let tickets = [ticket("m"), last("m.1"), last("m.2"), ticket("m.3")];
    assert_eq!(of(&tickets, &[], "m.1"), [blocker("m.3", 4)]);
    assert_eq!(ready(&tickets, &[], NOW), [id("m.3")]);
    let audited = [ticket("m"), last("m.1"), last("m.2"), closed("m.3")];
    assert_eq!(ready(&audited, &[], NOW), [id("m.1"), id("m.2")]);
}

#[test]
fn only_live_tickets_are_reported_and_in_the_order_given() {
    let tickets = [ticket("b"), closed("c"), ticket("a")];
    let reported: Vec<TicketId> = blockers(&tickets, &[], NOW)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(reported, [id("b"), id("a")]);
}

#[test]
fn a_closed_ticket_can_be_checked_against_the_live_set() {
    let tickets = [closed("a"), ticket("a.1")];
    let readiness = Readiness::new(&tickets, &[], NOW);
    assert_eq!(
        readiness.of(&tickets[0]),
        [blocker("a", 1), blocker("a.1", 5)]
    );
}

#[test]
fn an_edge_from_a_ticket_to_itself_holds_nothing_back() {
    let tickets = [ticket("a"), ticket("b")];
    let edges = [
        edge("a", "a", EdgeKind::Blocks),
        edge("b", "b", EdgeKind::Blocks),
    ];
    assert_eq!(of(&tickets, &edges, "a"), []);
    assert_eq!(ready(&tickets, &edges, NOW), [id("a"), id("b")]);
}

#[test]
fn a_meta_ticket_is_never_ready_but_its_copies_are() {
    let meta = Ticket {
        ticket_type: TicketType::Meta,
        every_hours: Some(6),
        ..ticket("MAB12")
    };
    assert_eq!(
        of(std::slice::from_ref(&meta), &[], "MAB12"),
        vec![blocker("MAB12", 1)]
    );
    let tickets = [meta, ticket("MAB12.1")];
    assert!(of(&tickets, &[], "MAB12.1").is_empty());
}
