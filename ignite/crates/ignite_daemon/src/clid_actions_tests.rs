//! Tests that each action `decide` can choose is carried out.

use ignite_core::cli::{Action, HoldReason, SlotState, TurnEnd};
use ignite_core::fakes::{FakeCliApp, ScriptedTurn};
use ignite_core::time::Timestamp;

use super::*;
use crate::dispatchd::RunningVerb;

#[test]
fn free_ends_the_slot_and_its_session_leaves_the_pool() {
    let rig = Rig::new();
    rig.push(clean("landed"));
    let session = rig.begin();
    rig.host.holders.lock().unwrap().clear();
    rig.then(Action::Free);
    rig.daemon.step().unwrap();
    let (end, facts) = rig.asked()[0].clone();
    assert_eq!(end, TurnEnd::Clean);
    assert!(!facts.ticket_held);
    assert_eq!(facts.last_text.as_deref(), Some("landed"));
    let slot = rig.slot();
    assert_eq!((slot.state, slot.session), (SlotState::Free, None));
    assert_eq!(slot.transcript.as_deref(), Some("t-1"));
    assert!(rig.pool.member(&session).unwrap().is_none());
    assert_eq!(rig.daemon.check(&session), SessionCheck::Gone);
    // The end is applied once.
    rig.daemon.step().unwrap();
    assert_eq!(rig.asked().len(), 1);
    assert_eq!(lock(&rig.effects.ends).len(), 1);
}

#[test]
fn release_gives_the_ticket_back_with_the_note() {
    let rig = Rig::new();
    rig.push(clean("stuck on the build"));
    let session = rig.begin();
    rig.then(Action::Release {
        note: "stuck on the build".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert!(rig.asked()[0].1.ticket_held);
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session.clone(), ticket(), "stuck on the build".to_owned())]
    );
    assert_eq!(rig.slot().state, SlotState::Free);
    assert!(rig.pool.member(&session).unwrap().is_none());
}

#[test]
fn resume_starts_the_next_turn_on_the_same_transcript() {
    let rig = Rig::new();
    rig.push(clean("first"));
    rig.keep_running(2);
    let session = rig.begin();
    rig.then(Action::Resume {
        prompt: "go on".to_owned(),
    });
    rig.daemon.step().unwrap();
    let starts = rig.turns.starts();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[1].1, 2);
    assert!(starts[1].2.args.ends_with(&[
        "--resume".to_owned(),
        "t-1".to_owned(),
        "go on".to_owned()
    ]));
    let row = rig.row(&session);
    assert_eq!((row.turn, row.state.as_str()), (2, "running"));
    // A clean end answered by resuming with nothing to deliver is the block text.
    assert_eq!(row.block_sent, 1);
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.starts().len(), 2);
}

#[test]
fn wait_for_verb_starts_the_next_turn_when_the_verb_ends() {
    let rig = Rig::new();
    rig.push(clean("waiting on the check"));
    let session = rig.begin();
    let verb = RunningVerb {
        verb: "check".to_owned(),
        started: Timestamp::from_millis(START),
        step: None,
    };
    rig.host
        .verbs
        .lock()
        .unwrap()
        .insert(session.clone(), vec![verb]);
    rig.then(Action::WaitForVerb {
        handle: "check".to_owned(),
        prompt: "`check` finished: run `ignite wait check`".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert_eq!(rig.asked()[0].1.running_verbs[0].command, "check");
    assert_eq!(rig.slot().state, SlotState::WaitingVerb);
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.starts().len(), 1);
    rig.host.verbs.lock().unwrap().clear();
    rig.keep_running(2);
    rig.daemon.step().unwrap();
    assert_eq!(
        rig.turns.prompts()[1],
        "`check` finished: run `ignite wait check`"
    );
    assert_eq!(rig.slot().state, SlotState::Running);
}

#[test]
fn back_off_starts_the_next_turn_at_its_time() {
    let rig = Rig::new();
    rig.push(ScriptedTurn {
        lines: vec![FakeCliApp::failed_line("overloaded")],
        exit: Some(1),
        hung: false,
    });
    let session = rig.begin();
    let at = Timestamp::from_millis(START).plus_seconds(30.0);
    rig.then(Action::BackOff {
        at,
        prompt: "try again".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert_eq!(rig.slot().state, SlotState::BackingOff);
    assert_eq!(rig.row(&session).backed_off, 1);
    rig.clock.advance(29.0);
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.starts().len(), 1);
    rig.clock.advance(1.0);
    rig.keep_running(2);
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.prompts()[1], "try again");
    // The next end knows the last one was answered with a back-off.
    lock(&rig.turns.live).clear();
    rig.daemon.step().unwrap();
    assert_eq!(
        rig.asked()[1].1.previous,
        ignite_core::cli::PreviousAnswer::BackedOff
    );
}

#[test]
fn hold_parks_the_slot_with_its_ticket_held() {
    let rig = Rig::new();
    rig.push(ScriptedTurn {
        lines: Vec::new(),
        exit: Some(1),
        hung: false,
    });
    let session = rig.begin();
    rig.then(Action::Hold {
        reason: HoldReason::NotLoggedIn,
        message: "Please run /login".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert!(matches!(
        rig.asked()[0].0,
        TurnEnd::Hold {
            reason: HoldReason::NoFirstLine,
            ..
        }
    ));
    assert_eq!(
        lock(&rig.effects.holds).clone(),
        [(
            "fake".to_owned(),
            HoldReason::NotLoggedIn,
            "Please run /login".to_owned()
        )]
    );
    let row = rig.row(&session);
    assert_eq!(row.state, "parked");
    assert!(row.park_reason.unwrap().contains("Please run /login"));
    assert!(lock(&rig.host.releases).is_empty());
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.starts().len(), 1);
}

#[test]
fn park_reports_and_starts_nothing() {
    let rig = Rig::new();
    rig.push(ScriptedTurn {
        lines: vec![FakeCliApp::failed_line("something new")],
        exit: Some(3),
        hung: false,
    });
    let session = rig.begin();
    rig.then(Action::Park {
        report: "an end nobody knows".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert!(matches!(rig.asked()[0].0, TurnEnd::Unknown { .. }));
    assert_eq!(
        lock(&rig.effects.reports).clone(),
        ["an end nobody knows".to_owned()]
    );
    assert_eq!(rig.row(&session).state, "parked");
    assert_eq!(rig.daemon.check(&session), SessionCheck::Idle);
    rig.daemon.step().unwrap();
    assert_eq!((rig.turns.starts().len(), rig.asked().len()), (1, 1));
    assert!(lock(&rig.host.releases).is_empty());
}
