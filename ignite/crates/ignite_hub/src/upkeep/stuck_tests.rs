//! What `stuck::stuck_epics` finds beyond a plain blocked epic: tickets no machine can take,
//! loops under no epic, and tickets held for hours.

use std::collections::BTreeSet;

use ignite_core::protocol::tickets::{EdgeKind, Operation, TicketType};
use ignite_core::traits::Clock;
use ignite_store::hub::{self, edges};

use super::Upkeep;
use super::tests::{all_tickets, apply, create, start, ticket};

fn label(id: &str, label: &str) -> Operation {
    Operation::Update {
        id: id.to_owned(),
        set: None,
        add_labels: vec![label.to_owned()],
        remove_labels: Vec::new(),
        set_meta: std::collections::BTreeMap::default(),
        unset_meta: Vec::new(),
    }
}

#[test]
fn an_epic_waiting_only_on_a_person_or_an_absent_machine_is_stuck() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let epic = apply(
        &mut conn,
        vec![create("Epic", TicketType::Epic, None, &[])],
        clock.now(),
    );
    let inside = apply(
        &mut conn,
        vec![create("Sign", TicketType::Task, Some(&epic[0]), &[])],
        clock.now(),
    );
    // Ready and with no needs: label, so a worker can take it.
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    apply(
        &mut conn,
        vec![label(&inside[0], "needs:person")],
        clock.now(),
    );
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(filed.title.contains(&epic[0]));
    assert!(filed.body.contains("needs:person"), "{}", filed.body);
    assert!(filed.labels.contains(&"graph-repair".to_owned()));
    assert!(filed.labels.contains(&"hub".to_owned()));
}

#[test]
fn a_loop_of_tickets_under_no_epic_is_reported_once() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let ids = apply(
        &mut conn,
        vec![
            create("A", TicketType::Task, None, &[]),
            create("B", TicketType::Task, None, &[]),
            create("C", TicketType::Task, None, &[]),
        ],
        clock.now(),
    );
    // The batch refuses a loop, so write the edges as old data or an import would have.
    edges::add(&conn, &ids[0], &ids[1], EdgeKind::Blocks).unwrap();
    edges::add(&conn, &ids[1], &ids[0], EdgeKind::Blocks).unwrap();
    edges::add(&conn, &ids[2], &ids[0], EdgeKind::Blocks).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(filed.labels.contains(&"graph-repair".to_owned()));
    assert!(filed.title.contains(&ids[0]) && filed.title.contains(&ids[1]));
    // C waits on the loop but is not in it.
    assert!(!filed.title.contains(&ids[2]));
    clock.advance(600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    assert_eq!(
        all_tickets(&conn, clock.now())
            .iter()
            .filter(|t| t.labels.contains(&"graph-repair".to_owned()))
            .count(),
        1
    );
}

#[test]
fn a_ticket_held_for_over_six_hours_is_reported_once_for_the_justiciar() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let ids = apply(
        &mut conn,
        vec![create("Long", TicketType::Task, None, &[])],
        clock.now(),
    );
    apply(
        &mut conn,
        vec![Operation::Claim { id: ids[0].clone() }],
        clock.now(),
    );
    clock.advance(5.0 * 3_600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    clock.advance(2.0 * 3_600.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(filed.labels.contains(&"justiciar".to_owned()));
    assert!(
        filed.body.contains("held for 7 hours by session test"),
        "{}",
        filed.body
    );
    clock.advance(3_600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    let labelled: BTreeSet<_> = all_tickets(&conn, clock.now())
        .into_iter()
        .filter(|t| t.labels.contains(&"justiciar".to_owned()))
        .map(|t| t.id)
        .collect();
    assert_eq!(labelled.len(), 1);
}

#[test]
fn an_epic_waiting_on_a_moving_epic_is_not_stuck() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let milestone = apply(
        &mut conn,
        vec![create("Milestone", TicketType::Epic, None, &[])],
        clock.now(),
    );
    let phase = apply(
        &mut conn,
        vec![create("Phase", TicketType::Epic, None, &[&milestone[0]])],
        clock.now(),
    );
    let last = apply(
        &mut conn,
        vec![create("Last phase", TicketType::Epic, None, &[&phase[0]])],
        clock.now(),
    );
    let work = apply(
        &mut conn,
        vec![
            create("Work", TicketType::Task, Some(&milestone[0]), &[]),
            create("Later", TicketType::Task, Some(&phase[0]), &[]),
            create("Last", TicketType::Task, Some(&last[0]), &[]),
        ],
        clock.now(),
    );
    // Later waits on the milestone epic, whose own ticket is ready, so the phase moves; Last
    // waits on the phase, so the last phase moves too.
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    // Once nothing under the milestone can move, all three epics are stuck.
    apply(
        &mut conn,
        vec![label(&work[0], "needs:person")],
        clock.now(),
    );
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    let titles: Vec<String> = report
        .tickets_filed
        .iter()
        .map(|id| ticket(&conn, id).title)
        .collect();
    assert!(titles.iter().any(|t| t.contains(&phase[0])), "{titles:?}");
    assert!(titles.iter().any(|t| t.contains(&milestone[0])), "{titles:?}");
    assert!(titles.iter().any(|t| t.contains(&last[0])), "{titles:?}");
}
