//! Tests of the reasons `why_nothing` gives, each on a snapshot `next_for` offers nothing for.

use super::*;
use crate::config::Os;
use crate::dispatch::next_for;
use crate::ids::{MachineId, SessionId, TicketId};
use crate::protocol::hub::IdleWorker;
use crate::time::Timestamp;

fn machine() -> MachineFree {
    MachineFree {
        room: None,
        machine: MachineId::new("jarvis").unwrap(),
        os: Os::Macos,
        idle_workers: vec![IdleWorker {
            session: SessionId::new("local_a").unwrap(),
            app: "claude".to_owned(),
            models: vec!["opus".to_owned(), "sonnet".to_owned()],
            model_ids: std::collections::BTreeMap::new(),
            default_model: None,
        }],
        free_ticket_trees: 4,
        free_cores: 1.0,
        admit_cores: Some(15.0),
        pressure: None,
        families: Vec::new(),
        name: Some("jarvis".to_owned()),
        justiciar_only: false,
        master_only: false,
        explain: true,
    }
}

fn ticket(name: &str, cores: f64) -> ReadyTicket {
    ReadyTicket {
        id: TicketId::new(name).unwrap(),
        kind: TicketKind::Ordinary,
        epic: None,
        priority: 2,
        chain: 0,
        created_at: Timestamp::from_millis(1_000),
        labels: Vec::new(),
        expected_cores: cores,
        families: Vec::new(),
        files: Vec::new(),
        saving: None,
        destinations: Vec::new(),
    }
}

/// The reason, after checking that dispatch really offers nothing for the snapshot.
fn why(free: &MachineFree, ready: &[ReadyTicket], order: &[Group]) -> String {
    assert_eq!(next_for(free, ready, order, None, &[]), None);
    why_nothing(free, ready, order)
}

const ALL: [Group; 1] = [None];

#[test]
fn an_empty_queue_says_to_file_tickets() {
    assert_eq!(
        why(&machine(), &[], &ALL),
        "no ready ticket is offered: file tickets, or release an epic"
    );
}

#[test]
fn tickets_too_large_for_the_room_left_wait_for_cores() {
    let ready = [ticket("TA", 2.5), ticket("TB", 4.0)];
    assert_eq!(
        why(&machine(), &ready, &ALL),
        "2 ready tickets wait for cores: the smallest expects 2.5, and 1.0 of the machine's \
         admitted cores are free"
    );
}

#[test]
fn tickets_for_another_machine_are_not_for_these_workers() {
    let mut other = ticket("TA", 0.5);
    other.destinations = vec!["multivac/codex".parse().unwrap()];
    assert_eq!(
        why(&machine(), &[other], &ALL),
        "1 ready ticket, none for this machine's idle workers (another owner or model)"
    );
}

#[test]
fn a_ticket_whose_epic_is_at_its_cap_waits_for_the_cap() {
    let mut capped = ticket("TA.1", 0.5);
    capped.epic = Some(TicketId::new("TA").unwrap());
    assert_eq!(
        why(&machine(), &[capped], &ALL),
        "1 ready ticket for this machine, all in epics at their --max-workers cap"
    );
}

#[test]
fn the_last_idle_worker_kept_for_justiciar_tickets_says_so() {
    let mut free = machine();
    free.justiciar_only = true;
    assert_eq!(
        why(&free, &[ticket("TA", 0.5)], &ALL),
        "the last idle worker is kept for justiciar tickets, and none is ready"
    );
}

#[test]
fn no_free_tree_comes_before_the_tickets() {
    let mut free = machine();
    free.free_ticket_trees = 0;
    assert_eq!(
        why(&free, &[ticket("TA", 0.5)], &ALL),
        "no ticket tree is free"
    );
}
