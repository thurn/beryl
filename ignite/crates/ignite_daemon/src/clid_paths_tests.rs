//! Tests of the paths that could lose or duplicate a worker's work: hand-offs that race the
//! loop and each other, messages a failed turn carried, a message for a worker inside
//! `ignite wait`, a parked slot whose ticket was given back, and urgent messages that end turns.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use ignite_core::cli::{Action, SlotState, TurnEnd};
use ignite_core::fakes::ScriptedTurn;

use super::*;

/// Adds `session` to the pool, as the dispatcher does after a hand-off.
fn join_pool(rig: &Rig, session: &SessionId) {
    rig.pool
        .put(&crate::pool::Member {
            session: session.clone(),
            app: "fake".to_owned(),
            down: false,
            models: Vec::new(),
            files: Vec::new(),
            added_at: rig.clock.now(),
            single_use: true,
        })
        .unwrap();
}

/// Hand-offs made from several threads while the `ignite-cli` loop runs each get a slot of
/// their own and a first turn. Before a hand-off claimed its slot and recorded its first turn
/// in one transaction, two of them could write the same slot, so one session lost its row
/// while its turn ran, and the loop could record a hand-off's first turn before the hand-off
/// did, which made the hand-off fail while its turn ran anyway.
#[test]
fn hand_offs_racing_each_other_and_the_loop_each_get_a_slot() {
    const HAND_OFFS: usize = 8;
    for _ in 0..20 {
        let rig = Rig::new();
        rig.keep_running(1);
        let stop = AtomicBool::new(false);
        let sessions: Vec<Result<SessionId>> = thread::scope(|scope| {
            let looping = scope.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    // A step can fail on a row a hand-off has not finished; the next one sees it.
                    let _ = rig.daemon.step();
                }
            });
            // Each hand-off is a reservation of its own: one the pool already took returns the
            // session it started.
            let handing: Vec<_> = (0..HAND_OFFS)
                .map(|n| {
                    let reservation = reservation(&format!("T.{}", n + 1), 1);
                    let daemon = &rig.daemon;
                    scope
                        .spawn(move || daemon.begin(&reservation, "m-1", Some("high"), None, false))
                })
                .collect();
            let sessions = handing.into_iter().map(|h| h.join().unwrap()).collect();
            stop.store(true, Ordering::Relaxed);
            looping.join().unwrap();
            sessions
        });
        let sessions: Vec<SessionId> = sessions.into_iter().map(Result::unwrap).collect();
        let slots = rig.daemon.slots("fake");
        assert_eq!(slots.len(), HAND_OFFS, "{slots:?}");
        for session in &sessions {
            let slot = slots
                .iter()
                .find(|s| s.session.as_ref() == Some(session))
                .unwrap_or_else(|| panic!("{session} lost its slot: {slots:?}"));
            assert_eq!((slot.turn, slot.state), (1, SlotState::Running));
        }
    }
}

/// A turn that printed nothing never showed its prompt to the worker, so a message it carried
/// reaches the next turn. Before, the message stayed marked delivered, and a slot resumed after
/// a CLI hold was told only that its last turn failed.
#[test]
fn a_message_carried_by_a_silent_turn_reaches_the_next_one() {
    let rig = Rig::new();
    rig.push(clean("working"));
    rig.push(ScriptedTurn {
        lines: Vec::new(),
        exit: Some(1),
        hung: false,
    });
    rig.keep_running(3);
    let session = rig.begin();
    rig.daemon
        .message(&session, "Message about T.1: rebase first", false)
        .unwrap();
    rig.then(Action::Resume {
        prompt: "go on".to_owned(),
    });
    rig.then(Action::Resume {
        prompt: "try again".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert!(rig.turns.prompts()[1].contains("rebase first"));
    rig.daemon.step().unwrap();
    assert_eq!(
        rig.turns.prompts()[2],
        "try again\n\nMessage about T.1: rebase first"
    );
}

/// A worker inside `ignite wait` is told a message waits, so it ends its turn and receives it.
/// Before, the wait ran on and the message waited with it. An urgent message ends the turn
/// instead.
#[test]
fn a_message_ends_the_workers_wait() {
    let rig = Rig::new();
    rig.keep_running(1);
    let session = rig.begin();
    rig.daemon
        .message(&session, "Message about T.1: rebase first", false)
        .unwrap();
    assert_eq!(
        lock(&rig.host.interrupts).clone(),
        ["T.1 has a message for you. End your turn to receive it.".to_owned()]
    );
    rig.daemon
        .message(&session, "Message about T.1: stop", true)
        .unwrap();
    assert_eq!(lock(&rig.host.interrupts).len(), 1);
    assert_eq!(lock(&rig.turns.ended).clone(), [(session.to_string(), 1)]);
}

/// A parked slot is freed once its session no longer holds its ticket, as when a person
/// releases it or its lease expires. Before, nothing looked at a parked slot again, so it
/// stayed taken for good.
#[test]
fn a_parked_slot_is_freed_when_its_ticket_is_given_back() {
    let rig = Rig::new();
    rig.push(clean("something new"));
    let session = rig.begin();
    rig.then(Action::Park {
        report: "an end nobody knows".to_owned(),
    });
    rig.daemon.step().unwrap();
    rig.daemon.step().unwrap();
    assert_eq!(rig.slot().state, SlotState::Parked);
    rig.host.holders.lock().unwrap().clear();
    rig.daemon.step().unwrap();
    assert!(rig.daemon.slots("fake").is_empty());
    assert!(rig.pool.member(&session).unwrap().is_none());
    assert_eq!(rig.turns.starts().len(), 1);
}

/// A turn ended to deliver an urgent message does not count toward the interrupted ends that
/// release a ticket. Before, five `tell --now` orders in a row released the ticket of a worker
/// that was only doing as it was told.
#[test]
fn a_turn_ended_for_an_urgent_message_is_not_counted_as_stuck() {
    let rig = Rig::new();
    rig.keep_running(1);
    rig.push(killed());
    let session = rig.begin();
    rig.pool
        .with_database(|conn| {
            conn.execute(
                "UPDATE cli_slots SET interrupted_in_row = 4 WHERE session = ?1",
                [session.as_str()],
            )
        })
        .unwrap();
    rig.daemon
        .message(&session, "Message about T.1: stop", true)
        .unwrap();
    rig.then(Action::Resume {
        prompt: "Your process was interrupted.".to_owned(),
    });
    rig.daemon.step().unwrap();
    let (end, facts) = rig.asked()[0].clone();
    assert_eq!(end, TurnEnd::Interrupted);
    assert_eq!(facts.interrupted_in_row, 1);
}

/// A slot in a state this build does not know (a newer build wrote it) reads as parked, but is
/// never freed, since its turn may be running.
#[test]
fn a_slot_in_an_unknown_state_is_not_freed() {
    let rig = Rig::new();
    rig.keep_running(1);
    let session = rig.begin();
    join_pool(&rig, &session);
    rig.pool
        .with_database(|conn| {
            conn.execute(
                "UPDATE cli_slots SET state = 'draining' WHERE session = ?1",
                [session.as_str()],
            )
        })
        .unwrap();
    rig.host.holders.lock().unwrap().clear();
    rig.daemon.step().unwrap();
    assert_eq!(rig.row(&session).state, "draining");
    assert!(rig.pool.member(&session).unwrap().is_some());
}

/// A first turn that a CLI hold parked before `ignite start` ran leaves a session that holds no
/// ticket tree yet. Its slot stays parked, so the hold's end resumes the hand-off.
#[test]
fn a_hand_off_parked_by_a_hold_before_its_claim_stays_parked() {
    let rig = Rig::new();
    rig.push(ScriptedTurn {
        lines: Vec::new(),
        exit: Some(1),
        hung: false,
    });
    let session = rig.try_begin().unwrap();
    rig.then(Action::Hold {
        reason: ignite_core::cli::HoldReason::NotLoggedIn,
        message: "Not logged in".to_owned(),
    });
    rig.daemon.step().unwrap();
    rig.daemon.step().unwrap();
    let slot = rig.slot();
    assert_eq!(slot.state, SlotState::Parked);
    assert_eq!(slot.session.as_ref(), Some(&session));
}

/// The health loop can resume a parked slot after the `ignite-cli` loop read it as parked. The
/// release check then writes nothing, and the session stays in the pool, since its next turn
/// is about to run.
#[test]
fn a_slot_resumed_meanwhile_keeps_its_session_in_the_pool() {
    let rig = Rig::new();
    rig.push(clean("something new"));
    let session = rig.begin();
    join_pool(&rig, &session);
    rig.then(Action::Park {
        report: "an end nobody knows".to_owned(),
    });
    rig.daemon.step().unwrap();
    let parked = rig.row(&session);
    assert_eq!(parked.state, "parked");
    rig.pool
        .with_database(|conn| {
            conn.execute(
                "UPDATE cli_slots SET state = 'backing_off' WHERE session = ?1",
                [session.as_str()],
            )
        })
        .unwrap();
    rig.host.holders.lock().unwrap().clear();
    rig.daemon.free_if_released(&parked).unwrap();
    assert_eq!(rig.row(&session).state, "backing_off");
    assert!(rig.pool.member(&session).unwrap().is_some());
}
