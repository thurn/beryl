//! Tests of the dispatch order, through `next_for` on whole snapshots.

use super::*;
use crate::admission;
use crate::config::Os;
use crate::ids::MachineId;
use crate::shares::{self, EpicDemand};

fn id(text: &str) -> TicketId {
    TicketId::new(text).unwrap()
}

fn session(text: &str) -> SessionId {
    SessionId::new(text).unwrap()
}

fn worker(name: &str, app: &str, models: &[&str]) -> IdleWorker {
    IdleWorker {
        session: session(name),
        app: app.to_owned(),
        models: models.iter().map(|m| (*m).to_owned()).collect(),
    }
}

fn machine(workers: Vec<IdleWorker>) -> MachineFree {
    MachineFree {
        machine: MachineId::new("jarvis").unwrap(),
        os: Os::Macos,
        idle_workers: workers,
        free_ticket_trees: 4,
        free_cores: 8.0,
        families: Vec::new(),
    }
}

fn ticket(name: &str, epic: Option<&str>) -> ReadyTicket {
    ReadyTicket {
        id: id(name),
        kind: TicketKind::Ordinary,
        epic: epic.map(id),
        priority: 2,
        chain: 0,
        created_at: Timestamp::from_millis(1_000),
        labels: Vec::new(),
        expected_cores: 0.5,
        families: Vec::new(),
        files: Vec::new(),
        saving: None,
    }
}

fn with_labels(mut t: ReadyTicket, labels: &[&str]) -> ReadyTicket {
    t.labels = labels.iter().map(|l| (*l).to_owned()).collect();
    t
}

fn ticket_of(handoff: Option<Handoff>) -> Option<String> {
    handoff.map(|h| h.ticket.to_string())
}

fn everyone() -> MachineFree {
    machine(vec![worker("local_a", "claude", &["opus", "sonnet"])])
}

const ALL: [Group; 1] = [None];

#[test]
fn a_regression_goes_first_and_skips_the_cores_test() {
    let mut free = everyone();
    free.free_cores = 0.0;
    let mut regression = ticket("Tr1", None);
    regression.kind = TicketKind::Regression;
    regression.expected_cores = 4.0;
    regression.priority = 3;
    let ready = [ticket("To1", None), regression];
    let answer = next_for(&free, &ready, &ALL, Some(Unit::Dollars), &[]);
    assert_eq!(ticket_of(answer), Some("Tr1".to_owned()));
}

#[test]
fn an_improvement_goes_next_only_while_under_its_share() {
    let mut improvement = ticket("Ti1", None);
    improvement.kind = TicketKind::Improvement;
    improvement.priority = 4;
    improvement.saving = Some((Unit::CoreHours, 12.0));
    let ready = [ticket("To1", None), improvement];
    let free = everyone();
    let due = next_for(&free, &ready, &ALL, Some(Unit::Dollars), &[]);
    assert_eq!(ticket_of(due), Some("Ti1".to_owned()));
    // Not due: the improvement waits its turn in its group, behind a higher priority.
    let not_due = next_for(&free, &ready, &ALL, None, &[]);
    assert_eq!(ticket_of(not_due), Some("To1".to_owned()));
}

#[test]
fn improvements_take_the_largest_saving_in_each_unit_in_turn() {
    let improvement = |name: &str, unit: Unit, amount: f64| {
        let mut t = ticket(name, None);
        t.kind = TicketKind::Improvement;
        t.saving = Some((unit, amount));
        t
    };
    let ready = [
        improvement("Td1", Unit::Dollars, 30.0),
        improvement("Td2", Unit::Dollars, 90.0),
        improvement("Tc1", Unit::CoreHours, 15.0),
    ];
    let names = |unit| {
        improvement_order(&ready, unit)
            .iter()
            .map(|t| t.id.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(Unit::Dollars), vec!["Td2", "Tc1", "Td1"]);
    assert_eq!(names(Unit::CoreHours), vec!["Tc1", "Td2", "Td1"]);
    assert_eq!(names(Unit::AgentHours), vec!["Tc1", "Td2", "Td1"]);
}

#[test]
fn the_group_furthest_below_its_share_goes_first() {
    let epics = [
        EpicDemand {
            epic: Some(id("Tq44s")),
            workers: Some(5),
            max_workers: None,
            ready: 3,
            held: 5,
        },
        EpicDemand {
            epic: Some(id("Tw8gv")),
            workers: Some(2),
            max_workers: None,
            ready: 3,
            held: 0,
        },
    ];
    let order = shares::epic_order(&epics);
    let mut urgent = ticket("Tq44s.1", Some("Tq44s"));
    urgent.priority = 0;
    let ready = [urgent, ticket("Tw8gv.1", Some("Tw8gv"))];
    let answer = next_for(&everyone(), &ready, &order, None, &[]);
    assert_eq!(ticket_of(answer), Some("Tw8gv.1".to_owned()));
    // A group the machine cannot serve is passed over for the next one.
    let ready = [
        ready[0].clone(),
        with_labels(ready[1].clone(), &["tier:fable"]),
    ];
    let answer = next_for(&everyone(), &ready, &order, None, &[]);
    assert_eq!(ticket_of(answer), Some("Tq44s.1".to_owned()));
}

#[test]
fn within_a_group_priority_then_chain_then_age_decide() {
    let mut old = ticket("Ta", None);
    old.created_at = Timestamp::from_millis(10);
    let mut long_chain = ticket("Tb", None);
    long_chain.chain = 3;
    let mut urgent = ticket("Tc", None);
    urgent.priority = 1;
    let free = everyone();
    let all = [old.clone(), long_chain.clone(), urgent];
    assert_eq!(
        ticket_of(next_for(&free, &all, &ALL, None, &[])),
        Some("Tc".to_owned())
    );
    let two = [old.clone(), long_chain];
    assert_eq!(
        ticket_of(next_for(&free, &two, &ALL, None, &[])),
        Some("Tb".to_owned())
    );
    let aged = [ticket("Tz", None), old];
    assert_eq!(
        ticket_of(next_for(&free, &aged, &ALL, None, &[])),
        Some("Ta".to_owned())
    );
}

#[test]
fn a_tools_ticket_is_admitted_where_a_third_kernel_ticket_waits() {
    // jarvis admits 13 cores and has 11.6 in progress, two kernel tickets among them.
    let mut free = everyone();
    free.free_cores = admission::free_cores(13.0, &[4.8, 4.6, 2.2]);
    let mut kernel = ticket("Tw8gv.21.80", None);
    kernel.priority = 1;
    kernel.expected_cores = 2.1;
    let mut tools = ticket("Ten2c.4", None);
    tools.expected_cores = 0.1;
    let ready = [kernel, tools];
    let answer = next_for(&free, &ready, &ALL, None, &[]);
    assert_eq!(ticket_of(answer), Some("Ten2c.4".to_owned()));
}

#[test]
fn a_ticket_needs_a_suitable_worker_a_tree_and_its_families() {
    let free = machine(vec![
        worker("local_codex", "codex", &["gpt"]),
        worker("local_claude", "claude", &["sonnet"]),
    ]);
    let sonnet = with_labels(ticket("Ts", None), &["tier:sonnet"]);
    let answer = next_for(&free, std::slice::from_ref(&sonnet), &ALL, None, &[]).unwrap();
    assert_eq!(answer.session, session("local_claude"));
    let opus = with_labels(ticket("To", None), &["tier:opus"]);
    assert_eq!(next_for(&free, &[opus], &ALL, None, &[]), None);
    let codex = with_labels(ticket("Tx", None), &["provider:codex"]);
    let answer = next_for(&free, &[codex], &ALL, None, &[]).unwrap();
    assert_eq!(answer.session, session("local_codex"));

    let mut no_trees = free.clone();
    no_trees.free_ticket_trees = 0;
    assert_eq!(
        next_for(&no_trees, std::slice::from_ref(&sonnet), &ALL, None, &[]),
        None
    );

    let mut limited = free.clone();
    limited.families = vec!["rust".to_owned()];
    let mut ui = sonnet.clone();
    ui.families = vec!["rust".to_owned(), "ui".to_owned()];
    assert_eq!(next_for(&limited, &[ui], &ALL, None, &[]), None);

    // A regression ticket names the operating system its check failed on.
    let other_os = with_labels(sonnet.clone(), &["needs:plan9"]);
    assert_eq!(next_for(&free, &[other_os], &ALL, None, &[]), None);
    let mac_only = with_labels(sonnet.clone(), &["needs:macos"]);
    assert!(next_for(&free, &[mac_only], &ALL, None, &[]).is_some());
}

#[test]
fn the_worker_whose_last_ticket_shared_most_files_gets_it() {
    let free = machine(vec![
        worker("local_a", "claude", &["opus"]),
        worker("local_b", "claude", &["opus"]),
    ]);
    let mut t = ticket("T1", None);
    t.files = vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()];
    let last = [
        (session("local_a"), vec!["src/z.rs".to_owned()]),
        (session("local_b"), vec!["src/b.rs".to_owned()]),
    ];
    let answer = next_for(&free, &[t.clone()], &ALL, None, &last).unwrap();
    assert_eq!(answer.session, session("local_b"));
    // With no history the first listed worker gets it: nothing waits for a particular worker.
    let answer = next_for(&free, &[t], &ALL, None, &[]).unwrap();
    assert_eq!(answer.session, session("local_a"));
}

#[test]
fn a_chain_continues_with_its_next_ready_ticket() {
    let first = with_labels(ticket("Tl.2", None), &["lane:port"]);
    let mut second = with_labels(ticket("Tl.3", None), &["lane:port"]);
    second.created_at = Timestamp::from_millis(5_000);
    let other = with_labels(ticket("Tm.1", None), &["lane:other"]);
    let ready = [second, other, first];
    assert_eq!(chain_next("port", &ready).unwrap().id, id("Tl.2"));
    assert!(chain_next("none", &ready).is_none());
}

#[test]
fn two_calls_on_the_same_snapshot_give_the_same_answer() {
    let free = machine(vec![
        worker("local_a", "claude", &["opus"]),
        worker("local_b", "claude", &["opus"]),
    ]);
    let ready: Vec<ReadyTicket> = (0..20)
        .map(|n| {
            let mut t = ticket(&format!("T{n}"), if n % 2 == 0 { None } else { Some("Tq") });
            t.priority = (n % 3) as u8;
            t.chain = (n % 4) as u32;
            t
        })
        .collect();
    let order = [Some(id("Tq")), None];
    let first = next_for(&free, &ready, &order, Some(Unit::CoreHours), &[]);
    let second = next_for(&free, &ready, &order, Some(Unit::CoreHours), &[]);
    assert!(first.is_some());
    assert_eq!(first, second);
}
