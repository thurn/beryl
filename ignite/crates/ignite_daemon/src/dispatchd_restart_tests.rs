//! A restarted dispatcher gives again what it had queued or given and not seen acknowledged.

use std::path::PathBuf;

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::tests::{START, detail, handoff_to, rig_on, session, ticket_id};
use super::*;

/// A database file for a test that restarts the daemon, removed when the test ends.
struct DbFile(PathBuf);

impl DbFile {
    fn new(name: &str) -> DbFile {
        let path =
            std::env::temp_dir().join(format!("ignite-dispatch-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        DbFile(path)
    }

    fn open(&self) -> rusqlite::Connection {
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
    assert_eq!(restarted.ack(again.id, "ok", "").exit, Exit::Success);
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
    first.dispatcher.chain(&first.ctx, &reservation);

    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    let queued = restarted.step().expect("queued before the restart");
    assert_eq!(handoff_to(&queued), ("local_w1", "T.1"));
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
