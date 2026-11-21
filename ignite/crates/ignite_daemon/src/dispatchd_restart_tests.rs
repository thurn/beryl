//! A restarted dispatcher gives again what it had queued or given and not seen acknowledged.

use std::path::PathBuf;

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::cli_tests::{cli, cli_with, tick};
use super::tests::{Rig, START, detail, handoff_to, lock, rig_on, session, ticket_id};
use super::*;
use crate::clid::CliPool;

/// A database file for a test that restarts the daemon, removed when the test ends.
pub(super) struct DbFile(PathBuf);

impl DbFile {
    pub(super) fn new(name: &str) -> DbFile {
        let path = ignite_core::test_files::temp_dir()
            .join(format!("ignite-dispatch-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        DbFile(path)
    }

    pub(super) fn open(&self) -> rusqlite::Connection {
        ignite_store::local::open(&self.0).unwrap()
    }
}

impl Drop for DbFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn a_handoff_given_before_a_restart_is_given_again_at_once() {
    let file = DbFile::new("given");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    assert_eq!(
        first
            .pool(&["add", "local_w1", "--app", "claude"], Some("local_d"))
            .exit,
        Exit::Success
    );
    first.reserve("T.1", "local_w1", &[]);
    let given = first.step().unwrap();
    handoff_to(&given);
    assert!(first.step().is_none());

    // The daemon restarts a few seconds later, long before the 2 minutes an unacknowledged
    // instruction normally waits. Before the queue was saved, the new daemon knew nothing of it
    // and the worker sat idle until the hub lapsed the reservation.
    let now = first.clock.now().plus_seconds(5.0);
    let restarted = rig_on(&[], file.open(), now);
    let again = restarted.step().expect("given again without waiting");
    assert_eq!(again, given);
    assert_eq!(restarted.handed(&again).exit, Exit::Success);
    assert_eq!(restarted.state_of("local_w1"), "reserved");
    assert!(restarted.step().is_none());
}

#[test]
fn a_queued_handoff_and_the_instruction_ids_survive_a_restart() {
    let file = DbFile::new("queued");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    assert_eq!(
        first
            .pool(&["add", "local_w1", "--app", "claude"], Some("local_d"))
            .exit,
        Exit::Success
    );
    let reservation = Reservation {
        ticket: ticket_id("T.1"),
        machine: first.ctx.machine.clone(),
        session: session("local_w1"),
        expires_at: first.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    };
    first
        .hub
        .insert_ticket(detail("T.1", "open", None, &[], ""));
    first
        .dispatcher
        .chain(&first.ctx, &ticket_id("T.0"), &reservation, true);

    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    let queued = restarted.step().expect("queued before the restart");
    assert_eq!(handoff_to(&queued), ("local_w1", "T.1"));
    assert!(queued.clear);
    // A new instruction does not reuse the ID of the one that was queued.
    restarted.dispatcher.forget(&session("nobody"));
    let id = {
        let mut state = restarted.dispatcher.state();
        Dispatcher::next_id(&mut state)
    };
    assert!(id > queued.id);
}

#[test]
fn a_refusal_and_a_silent_workers_stall_survive_a_restart() {
    let file = DbFile::new("refused");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    assert_eq!(
        first
            .pool(&["add", "local_w1", "--app", "claude"], Some("local_d"))
            .exit,
        Exit::Success
    );
    first.reserve("T.1", "local_w1", &[]);
    let instruction = first.step().unwrap();
    first.ack(instruction.id, "failed", "busy");
    {
        let mut state = first.dispatcher.state();
        state.stalls.insert(
            session("local_w1"),
            Stall {
                checked_at: first.clock.now(),
                messaged_at: Some(first.clock.now()),
                stuck: None,
                woken: false,
            },
        );
    }
    first.dispatcher.save();

    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    let state = restarted.dispatcher.state();
    assert!(state.refused.contains_key(&session("local_w1")));
    assert!(
        state
            .stalls
            .get(&session("local_w1"))
            .unwrap()
            .messaged_at
            .is_some()
    );
}

#[test]
fn a_worker_that_died_before_a_restart_is_checked_and_released() {
    let file = DbFile::new("died");
    let first = rig_on(&["local_w1"], file.open(), Timestamp::from_millis(START));
    let claimed_at = first.clock.now();
    first.hold("local_w1", "T.1");
    lock(&first.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "start T.1".to_owned(),
            since: claimed_at,
            running: false,
        },
    );
    assert!(first.step().is_none());

    // The daemon restarts 15 minutes after the claim. The server's record of commands is lost,
    // but the claims' saved clock still says when the worker last ran one. The worker is dead
    // and never runs another command.
    let restarted = rig_on(&[], file.open(), claimed_at.plus_seconds(15.0 * 60.0));
    restarted.hold("local_w1", "T.1");
    lock(&restarted.host.quiet).insert(session("local_w1"), claimed_at);
    assert!(restarted.step().is_none());
    restarted.clock.advance(14.0 * 60.0);
    assert!(restarted.step().is_none(), "not yet 30 minutes");
    restarted.clock.advance(60.0);
    let check = restarted
        .step()
        .expect("checked 30 minutes after the claim");
    assert_eq!(check.action, DispatchAction::Check);
    assert_eq!(check.session, Some(session("local_w1")));
    restarted.ack(check.id, "gone", "");
    assert_eq!(
        lock(&restarted.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
}

/// A tell delivered before a restart is not delivered again after it. Before the delivered
/// tells were saved, the restarted dispatcher re-sent a ticket's latest tell at the next change
/// to the ticket, and for a CLI worker an old `tell --now` ended its running turn again.
#[test]
fn a_tell_delivered_before_a_restart_is_not_delivered_again() {
    let file = DbFile::new("tells");
    let told = super::tell_tests::claimed(
        "T.1",
        "local_w1",
        &[(1, "2027-01-02T00:00:00.000Z", "Land when the gate passes.")],
        "2027-01-01T00:00:00.000Z",
    );
    let first = rig_on(&["local_w1"], file.open(), Timestamp::from_millis(START));
    first.hold("local_w1", "T.1");
    assert!(first.step().is_none());
    first.dispatcher.poll_feed(&first.ctx, 0).unwrap();
    first.hub.insert_ticket(told.clone());
    first.hub.push_change("ticket", Some(ticket_id("T.1")));
    first.dispatcher.poll_feed(&first.ctx, 0).unwrap();
    let message = first.step().unwrap();
    assert_eq!(message.action, DispatchAction::Message);
    assert_eq!(first.ack(message.id, "ok", "").exit, Exit::Success);

    let restarted = rig_on(&["local_w1"], file.open(), first.clock.now());
    restarted.hold("local_w1", "T.1");
    assert!(restarted.step().is_none());
    restarted.dispatcher.poll_feed(&restarted.ctx, 0).unwrap();
    restarted.hub.insert_ticket(told);
    restarted.hub.push_change("ticket", Some(ticket_id("T.1")));
    restarted.dispatcher.poll_feed(&restarted.ctx, 0).unwrap();
    let next = restarted.step();
    assert!(
        next.as_ref()
            .is_none_or(|i| i.action != DispatchAction::Message),
        "{next:?}"
    );
}

/// The hand-off of T.1 to CLI slot 1, queued and saved as `push_handoff` leaves it, with the
/// daemon stopped before the `Begin` it queued ran. Returns the reservation the hub made.
fn saved_cli_handoff(rig: &Rig) -> Reservation {
    let reservation = Reservation {
        ticket: ticket_id("T.1"),
        machine: rig.ctx.machine.clone(),
        session: session("cli-claude-slot-1"),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    };
    let reserved = Reserved {
        ticket: reservation.ticket.clone(),
        expires_at: reservation.expires_at,
        code: Some(reservation.code.clone()),
        tier: Some("opus".to_owned()),
        model: Some("opus".to_owned()),
        effort: "high".to_owned(),
        app: "claude".to_owned(),
    };
    let prompt = "Run `ignite start T.1 --reservation 7f3a`".to_owned();
    let mut state = rig.dispatcher.state();
    Dispatcher::push_handoff(&mut state, &reservation.session, reserved, prompt, false);
    drop(state);
    rig.dispatcher.save();
    reservation
}

/// The sessions holding a reservation in `rig`'s dispatcher.
fn reserved_sessions(rig: &Rig) -> Vec<String> {
    let state = rig.dispatcher.state();
    state
        .reserved
        .keys()
        .map(|s| s.as_str().to_owned())
        .collect()
}

#[test]
fn a_cli_handoff_saved_before_its_begin_ran_starts_one_session_after_a_restart() {
    let file = DbFile::new("cli-unbegun");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    saved_cli_handoff(&first);

    // Before the queued `Begin` was saved, the restarted daemon restored the reservation under
    // the slot's placeholder and started nothing, so the ticket waited for the hub to lapse it.
    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    let fake = cli(&restarted, 2);
    tick(&restarted);
    let begun = fake.begun();
    assert_eq!(begun.len(), 1);
    assert_eq!(begun[0].0.code, "7f3a");
    assert_eq!(begun[0].0.session, session("cli-claude-slot-1"));
    assert_eq!(reserved_sessions(&restarted), ["cli-claude-00000001"]);

    // The hand-off ran, so neither another step nor another restart runs it again.
    tick(&restarted);
    let again = rig_on(&[], file.open(), restarted.clock.now().plus_seconds(5.0));
    cli_with(&again, 2, fake.clone());
    tick(&again);
    assert_eq!(fake.begun().len(), 1);
    assert_eq!(reserved_sessions(&again), ["cli-claude-00000001"]);
}

#[test]
fn a_cli_handoff_begun_before_its_session_was_saved_starts_no_second_session() {
    let file = DbFile::new("cli-begun");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    let fake = cli(&first, 2);
    let reservation = saved_cli_handoff(&first);
    // The CLI pool started the session, and the daemon died before it saved the reservation
    // under the new session.
    let started = fake
        .begin(&reservation, "opus", Some("high"), None, false)
        .unwrap();
    assert_eq!(reserved_sessions(&first), ["cli-claude-slot-1"]);

    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    cli_with(&restarted, 2, fake.clone());
    tick(&restarted);
    assert_eq!(fake.begun().len(), 1, "no second session started");
    assert_eq!(reserved_sessions(&restarted), [started.as_str()]);
}

#[test]
fn a_cli_handoff_whose_reservation_lapsed_before_the_restart_starts_nothing() {
    let file = DbFile::new("cli-lapsed");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    saved_cli_handoff(&first);

    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(601.0));
    let fake = cli(&restarted, 2);
    tick(&restarted);
    assert!(fake.begun().is_empty());
    assert!(reserved_sessions(&restarted).is_empty());
}
