//! A restarted daemon recovers pending CLI starts and remembers the tells it delivered.

use std::path::PathBuf;

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::cli_tests::{cli, cli_with, tick};
use super::tests::{Rig, START, lock, rig_on, session, ticket_id};
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
fn a_tell_delivered_before_a_restart_is_not_delivered_again() {
    let file = DbFile::new("tells");
    let told = super::tell_tests::claimed(
        "T.1",
        "cli-claude-00000001",
        &[(1, "2027-01-02T00:00:00.000Z", "Land when the gate passes.")],
        "2027-01-01T00:00:00.000Z",
    );
    let first = rig_on(
        &["cli-claude-00000001"],
        file.open(),
        Timestamp::from_millis(START),
    );
    first.hold("cli-claude-00000001", "T.1");
    let fake = cli(&first, 1);
    tick(&first);
    first.dispatcher.poll_feed(&first.ctx, 0).unwrap();
    first.hub.insert_ticket(told.clone());
    first.hub.push_change("ticket", Some(ticket_id("T.1")));
    first.dispatcher.poll_feed(&first.ctx, 0).unwrap();
    assert_eq!(fake.sent().len(), 1);

    let restarted = rig_on(&["cli-claude-00000001"], file.open(), first.clock.now());
    restarted.hold("cli-claude-00000001", "T.1");
    cli_with(&restarted, 1, fake.clone());
    tick(&restarted);
    restarted.dispatcher.poll_feed(&restarted.ctx, 0).unwrap();
    restarted.hub.insert_ticket(told);
    restarted.hub.push_change("ticket", Some(ticket_id("T.1")));
    restarted.dispatcher.poll_feed(&restarted.ctx, 0).unwrap();
    assert_eq!(fake.sent().len(), 1);
}

/// The reservation for T.1 in CLI slot 1, saved before it starts.
fn saved_cli_handoff(rig: &Rig) -> Reservation {
    let reservation = Reservation {
        master_blocker: false,
        provider: Some("claude".to_owned()),
        ticket: ticket_id("T.1"),
        machine: rig.ctx.machine.clone(),
        session: session("reservation-1"),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    };
    let reserved = Reserved {
        ticket: reservation.ticket.clone(),
        expires_at: reservation.expires_at,
        code: Some(reservation.code.clone()),
        model: Some("opus".to_owned()),
        effort: "high".to_owned(),
        app: "claude".to_owned(),
        master_blocker: reservation.master_blocker,
    };
    let mut state = rig.dispatcher.state();
    Dispatcher::begin_in_slot(&mut state, &reservation.session, reserved, None, false);
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
    assert_eq!(begun[0].0.session, session("reservation-1"));
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
    assert_eq!(reserved_sessions(&first), ["reservation-1"]);

    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    cli_with(&restarted, 2, fake.clone());
    *lock(&restarted.host.worker_capacity) = (2, true, Some(512));
    tick(&restarted);
    assert_eq!(fake.begun().len(), 1, "no second session started");
    assert!(restarted.hub.refused_reservations().is_empty());
    assert_eq!(reserved_sessions(&restarted), [started.as_str()]);
}

#[test]
fn a_provider_held_after_reservation_returns_it_without_parking_a_worker() {
    let rig = super::tests::rig(&[]);
    let fake = cli(&rig, 2);
    let reservation = saved_cli_handoff(&rig);
    fake.set_hold(
        "claude",
        Some(ignite_core::cli::Hold {
            app: "claude".to_owned(),
            reason: ignite_core::cli::HoldReason::NotLoggedIn,
            since: rig.clock.now(),
            cli_version: None,
            message: "login expired".to_owned(),
        }),
    );
    tick(&rig);
    assert!(fake.begun().is_empty());
    assert!(reserved_sessions(&rig).is_empty());
    assert_eq!(rig.hub.refused_reservations(), [reservation]);
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

#[test]
fn an_upgrade_recovers_a_cli_start_from_the_old_daemon_database() {
    let db = ignite_store::local::open_in_memory().unwrap();
    let now = Timestamp::from_millis(START);
    let expires_at = now.plus_seconds(600.0);
    let reservation = serde_json::json!({
        "ticket": "T.1",
        "expires_at": expires_at,
        "code": "7f3a",
        "tier": "opus",
        "model": "opus",
        "effort": "high",
        "app": "claude"
    });
    db.execute(
        "INSERT INTO dispatch_queue (kind, key, body) VALUES ('reserved', ?1, ?2)",
        ["cli-claude-slot-1", &reservation.to_string()],
    )
    .unwrap();
    let start = serde_json::json!({
        "Begin": {"slot": "cli-claude-slot-1", "reserved": reservation, "resume": null}
    });
    db.execute(
        "INSERT INTO dispatch_queue (kind, key, body) VALUES ('cli_begin', '0', ?1)",
        [start.to_string()],
    )
    .unwrap();

    let rig = rig_on(&[], db, now);
    let fake = cli(&rig, 1);
    tick(&rig);

    let begun = fake.begun();
    assert_eq!(begun.len(), 1);
    assert_eq!(begun[0].0.code, "7f3a");
    assert_eq!(begun[0].1, "opus");
    assert_eq!(reserved_sessions(&rig), ["cli-claude-00000001"]);
}
