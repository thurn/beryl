use std::collections::BTreeMap;

use ignite_core::ids::{CheckName, Commit, MachineId};
use ignite_core::protocol::tickets::{Operation, Status, TicketFields};
use ignite_core::traits::Clock;
use ignite_store::hub::{self, tickets};

use super::{ExplorationFind, find};
use crate::upkeep::filing::{Change, UPKEEP};
use crate::upkeep::tests::{all_tickets, apply, start, ticket};

fn finding(family: &str, scenario: &str, output: &str) -> ExplorationFind {
    let check = format!("{family}:{scenario}");
    ExplorationFind {
        check: CheckName::new(&check).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        host: "jarvis".to_owned(),
        commit: Commit::new("abcd").unwrap(),
        replay: format!("ignite test {check}"),
        output: output.to_owned(),
    }
}

#[test]
fn exploration_limits_pending_tickets_and_keeps_each_replay() {
    let now = start().now();
    let mut conn = hub::open_in_memory().unwrap();
    let first = find(&mut conn, UPKEEP, &finding("fuzz", "17", "panic"), now).unwrap();
    assert_eq!(first.change, Change::Created);
    assert!(first.ticket.starts_with('A'));
    let other = finding("fuzz", "94", "wrong result");
    let second = find(&mut conn, UPKEEP, &other, now).unwrap();
    assert_eq!(second.ticket, first.ticket);
    assert_eq!(second.change, Change::Commented);
    assert_eq!(
        find(&mut conn, UPKEEP, &other, now).unwrap().change,
        Change::Unchanged
    );

    for family in ["drift", "oracle"] {
        let filed = find(&mut conn, UPKEEP, &finding(family, "1", "failed"), now).unwrap();
        assert_eq!(filed.change, Change::Created);
    }
    let extra = find(
        &mut conn,
        UPKEEP,
        &finding("simulation", "9", "mismatch"),
        now,
    )
    .unwrap();
    assert_eq!(extra.change, Change::Commented);
    assert_ne!(extra.ticket, first.ticket);
    let again = find(
        &mut conn,
        UPKEEP,
        &finding("simulation", "10", "timeout"),
        now,
    )
    .unwrap();
    assert_eq!(again.ticket, extra.ticket);
    assert_eq!(all_tickets(&conn, now).len(), 3);
    let mixed = ticket(&conn, &extra.ticket);
    assert!(mixed.labels.contains(&"found-by:simulation".to_owned()));
    assert_eq!(mixed.title, "Investigate exploration findings");
    assert_eq!(mixed.status, Status::PendingApproval);
    let comments = tickets::comments(&conn, &extra.ticket).unwrap();
    assert!(
        comments[0]
            .text
            .contains("replay: ignite test simulation:9")
    );
    assert!(
        comments[1]
            .text
            .contains("replay: ignite test simulation:10")
    );

    let filed = ticket(&conn, &first.ticket);
    assert_eq!(filed.priority, 2);
    assert_eq!(filed.labels, ["found-by:fuzz", "hub"]);
    assert!(filed.body.contains("replay: ignite test fuzz:17"));
    let comments = tickets::comments(&conn, &first.ticket).unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].text.contains("replay: ignite test fuzz:94"));
    assert!(comments[0].text.contains("wrong result"));
}

#[test]
fn approving_an_exploration_ticket_keeps_its_findings_fixed() {
    let now = start().now();
    let mut conn = hub::open_in_memory().unwrap();
    let report = finding("fuzz", "17", "panic");
    let first = find(&mut conn, UPKEEP, &report, now).unwrap();
    apply(
        &mut conn,
        vec![Operation::Update {
            id: first.ticket.clone(),
            set: Some(TicketFields {
                status: Some(Status::Open),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::new(),
            unset_meta: Vec::new(),
        }],
        now,
    );
    let repeat = find(&mut conn, UPKEEP, &report, now).unwrap();
    assert_eq!(repeat.ticket, first.ticket);
    assert_eq!(repeat.change, Change::Unchanged);

    let later = find(
        &mut conn,
        UPKEEP,
        &finding("fuzz", "17", "wrong result"),
        now,
    )
    .unwrap();
    assert_eq!(later.change, Change::Created);
    assert_ne!(later.ticket, first.ticket);
    assert_eq!(ticket(&conn, &later.ticket).status, Status::PendingApproval);
    assert!(tickets::comments(&conn, &first.ticket).unwrap().is_empty());
}
