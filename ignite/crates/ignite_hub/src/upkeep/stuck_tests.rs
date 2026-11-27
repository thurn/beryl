//! What `stuck::stuck_epics` finds beyond a plain blocked epic: tickets no machine can take,
//! loops under no epic, and tickets held for hours.

use std::collections::BTreeSet;

use ignite_core::protocol::tickets::{EdgeKind, Operation, Status, TicketType};
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
fn an_epic_whose_children_await_approval_is_not_stuck() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let epic = apply(
        &mut conn,
        vec![create("Epic", TicketType::Epic, None, &[])],
        clock.now(),
    );
    let mut child = create("Child", TicketType::Task, Some(&epic[0]), &[]);
    if let Operation::Create { fields, .. } = &mut child {
        fields.status = Some(Status::PendingApproval);
    }
    apply(&mut conn, vec![child], clock.now());
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
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
        vec![create("Phase", TicketType::Epic, None, &[])],
        clock.now(),
    );
    let last = apply(
        &mut conn,
        vec![create("Last phase", TicketType::Epic, None, &[])],
        clock.now(),
    );
    // Restore the external dependencies as they would appear in an older database.
    edges::add(&conn, &phase[0], &milestone[0], EdgeKind::Blocks).unwrap();
    edges::add(&conn, &last[0], &phase[0], EdgeKind::Blocks).unwrap();
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
    assert!(
        titles.iter().any(|t| t.contains(&milestone[0])),
        "{titles:?}"
    );
    assert!(titles.iter().any(|t| t.contains(&last[0])), "{titles:?}");
}

#[test]
fn a_loop_through_a_parent_link_is_reported() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let ids = apply(
        &mut conn,
        vec![
            create("A", TicketType::Task, None, &[]),
            create("Parent", TicketType::Task, None, &[]),
        ],
        clock.now(),
    );
    let child = apply(
        &mut conn,
        vec![create("Child", TicketType::Task, Some(&ids[1]), &[])],
        clock.now(),
    );
    // A waits on Child, and Child waits on A through its parent's dependency on A.
    edges::add(&conn, &ids[0], &child[0], EdgeKind::Blocks).unwrap();
    edges::add(&conn, &ids[1], &ids[0], EdgeKind::Blocks).unwrap();
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(filed.labels.contains(&"graph-repair".to_owned()));
    assert!(
        filed.title.contains(&ids[0]) && filed.title.contains(&child[0]),
        "{}",
        filed.title
    );
}

/// A tip that waits on checks only another system's machines can run holds `master` back: its
/// ticket is filed open, and a ticket still waiting for a person that holds back one of those
/// checks is released.
#[test]
fn a_tip_waiting_on_checks_files_open_and_releases_tickets_holding_them() {
    use ignite_core::config::Os;
    use ignite_core::ids::{CheckName, Commit, MachineId};
    use ignite_core::protocol::hub::{IncompleteReason, IncompleteReport, TipWaiting};

    use super::filing::UPKEEP;
    use super::stuck;

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let held = CheckName::new("rust:windows_paths").unwrap();
    // Each report's output differs once check names are taken out, so each is its own cause.
    let incomplete = |check: &CheckName, output: &str| IncompleteReport {
        check: check.clone(),
        machine: MachineId::new("m-3").unwrap(),
        host: "cygnus".to_owned(),
        platform: Some(Os::Windows),
        attempts: 2,
        reason: IncompleteReason::Hung,
        commit: Commit::new("abcd").unwrap(),
        output: output.to_owned(),
        checks: vec![check.clone()],
        count: 1,
        product: None,
        holds_tip: false,
    };
    let waited = stuck::incomplete(
        &mut conn,
        UPKEEP,
        &incomplete(&held, "paths hung"),
        clock.now(),
    )
    .unwrap();
    let other = CheckName::new("rust:other").unwrap();
    let unrelated = stuck::incomplete(
        &mut conn,
        UPKEEP,
        &incomplete(&other, "other hung"),
        clock.now(),
    )
    .unwrap();
    assert_ne!(waited.ticket, unrelated.ticket);
    for id in [&waited.ticket, &unrelated.ticket] {
        assert_eq!(ticket(&conn, id).status, Status::PendingApproval);
    }

    let filed = stuck::tip_waiting(
        &mut conn,
        UPKEEP,
        &TipWaiting {
            commit: Commit::new("abcd").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            platform: Os::Windows,
            checks: vec![held],
            waited_s: 3_600.0,
            unrunnable: Vec::new(),
        },
        clock.now(),
    )
    .unwrap();
    assert_eq!(ticket(&conn, &filed[0].ticket).status, Status::Open);
    assert_eq!(ticket(&conn, &waited.ticket).status, Status::Open);
    assert_eq!(
        ticket(&conn, &unrelated.ticket).status,
        Status::PendingApproval
    );
}

/// Claims a ticket for session `s-1` and returns the ticket's ID.
fn held_by_s1(conn: &mut rusqlite::Connection, now: ignite_core::time::Timestamp) -> String {
    let work = apply(conn, vec![create("Work", TicketType::Task, None, &[])], now);
    conn.execute(
        "UPDATE tickets SET status = 'in_progress', assignee = 's-1', session = 's-1',
         claimed_at = ?2 WHERE id = ?1",
        rusqlite::params![work[0], now.to_string()],
    )
    .unwrap();
    work[0].clone()
}

fn set_title(conn: &rusqlite::Connection, title: &str, now: ignite_core::time::Timestamp) {
    hub::session_titles::record(conn, "s-1", title, now).unwrap();
}

fn set_worker(conn: &rusqlite::Connection, state: &str, since: ignite_core::time::Timestamp) {
    conn.execute("DELETE FROM workers", []).unwrap();
    conn.execute(
        "INSERT INTO workers (machine, session, app, role, state, state_since)
         VALUES ('m', 's-1', 'claude', 'worker', ?1, ?2)",
        rusqlite::params![state, since.to_string()],
    )
    .unwrap();
}

#[test]
fn a_ticket_held_by_a_stopped_session_is_reported_after_thirty_minutes() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    held_by_s1(&mut conn, clock.now());
    set_title(&conn, "⛔ [T1] stopped", clock.now());
    clock.advance(20.0 * 60.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    clock.advance(15.0 * 60.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(filed.labels.contains(&"justiciar".to_owned()));
    assert!(
        filed.body.contains("held 35 min by a session titled ⛔"),
        "{}",
        filed.body
    );
    clock.advance(600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
}

#[test]
fn a_ticket_held_by_an_idle_session_is_reported_after_thirty_minutes() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    held_by_s1(&mut conn, clock.now());
    set_title(&conn, "⚒️ [T1] working", clock.now());
    set_worker(&conn, "idle", clock.now());
    clock.advance(20.0 * 60.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    clock.advance(15.0 * 60.0);
    let report = upkeep.pass(&mut conn, clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let filed = ticket(&conn, &report.tickets_filed[0]);
    assert!(
        filed.body.contains("held 35 min by an idle session"),
        "{}",
        filed.body
    );
}

#[test]
fn a_ticket_held_by_a_working_session_waits_six_hours() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    held_by_s1(&mut conn, clock.now());
    set_title(&conn, "⚒️ [T1] working", clock.now());
    set_worker(&conn, "working", clock.now());
    clock.advance(5.0 * 3_600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    clock.advance(2.0 * 3_600.0);
    assert_eq!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .len(),
        1
    );
}

#[test]
fn a_ticket_reclaimed_ten_minutes_ago_is_not_held_even_if_first_claimed_long_ago() {
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let work = apply(
        &mut conn,
        vec![create("Work", TicketType::Task, None, &[])],
        clock.now(),
    );
    let claim = || Operation::Claim {
        id: work[0].clone(),
    };
    apply(&mut conn, vec![claim()], clock.now());
    apply(
        &mut conn,
        vec![Operation::Close {
            id: work[0].clone(),
            reason: "paused".to_owned(),
            include_ready: false,
        }],
        clock.now(),
    );
    clock.advance(8.0 * 3_600.0);
    apply(&mut conn, vec![claim()], clock.now());
    clock.advance(10.0 * 60.0);
    assert_eq!(ticket(&conn, &work[0]).status, Status::InProgress);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
}

#[test]
fn a_held_ticket_closed_by_a_session_waits_for_cooldown_before_refiling() {
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
    clock.advance(7.0 * 3_600.0);
    let first = upkeep.pass(&mut conn, clock.now()).unwrap().tickets_filed;
    assert_eq!(first.len(), 1);
    apply(
        &mut conn,
        vec![Operation::Close {
            id: first[0].clone(),
            reason: "worker is actively running ignite check".to_owned(),
            include_ready: false,
        }],
        clock.now(),
    );
    clock.advance(3_600.0);
    assert!(
        upkeep
            .pass(&mut conn, clock.now())
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    clock.advance(3_660.0);
    let second = upkeep.pass(&mut conn, clock.now()).unwrap().tickets_filed;
    assert_eq!(second.len(), 1);
    assert_ne!(second[0], first[0]);
}

#[test]
fn a_held_ticket_closed_by_upkeep_refiles_without_cooldown() {
    use super::filing::UPKEEP;
    use ignite_core::protocol::tickets::BatchRequest;

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
    clock.advance(7.0 * 3_600.0);
    let first = upkeep.pass(&mut conn, clock.now()).unwrap().tickets_filed;
    assert_eq!(first.len(), 1);
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Close {
            id: first[0].clone(),
            reason: "cleared".to_owned(),
            include_ready: false,
        }],
    };
    hub::batch::apply(
        &mut conn,
        &request,
        UPKEEP.actor,
        UPKEEP.machine,
        clock.now(),
    )
    .unwrap();
    clock.advance(5.0 * 60.0);
    let second = upkeep.pass(&mut conn, clock.now()).unwrap().tickets_filed;
    assert_eq!(second.len(), 1);
}
