//! How often a silent worker, or one whose command is stuck, is checked: once per silence or
//! stuck command, whatever the answer.

use super::cli_tests::{cli, tick};
use super::stall::{STUCK_COMMAND_S, STUCK_STEP_S};
use super::tests::{lock, rig, session, ticket_id};
use super::*;
use crate::clid::{FakeCliPool, SessionCheck};
use ignite_core::traits::Clock;

/// A rig whose one worker holds `T.1` and last ran a command at the start.
fn silent_worker() -> (super::tests::Rig, Arc<FakeCliPool>) {
    let rig = rig(&["cli-claude-00000001"]);
    let fake = cli(&rig, 1);
    rig.hold("cli-claude-00000001", "T.1");
    lock(&rig.host.commands).insert(
        session("cli-claude-00000001"),
        SessionCommand {
            command: "test x".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    tick(&rig);
    rig.clock.advance(SILENT_S);
    (rig, fake)
}

#[test]
fn a_worker_answered_running_is_checked_again_only_after_another_silence() {
    let (rig, fake) = silent_worker();
    fake.set_check(&session("cli-claude-00000001"), SessionCheck::Running);
    tick(&rig);
    // The silence has not changed, but the worker was just seen running.
    rig.clock.advance(30.0);
    tick(&rig);
    assert!(fake.sent().is_empty());
    assert!(lock(&rig.host.releases).is_empty());
    rig.clock.advance(SILENT_S - 30.0);
    fake.set_check(&session("cli-claude-00000001"), SessionCheck::Idle);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1);
}

#[test]
fn a_release_that_fails_is_retried_at_the_next_check_without_another_reminder() {
    let (rig, fake) = silent_worker();
    *lock(&rig.host.fail_release) = true;
    fake.set_check(&session("cli-claude-00000001"), SessionCheck::Idle);
    tick(&rig);
    assert_eq!(fake.sent()[0].text, CONTINUE);
    rig.clock.advance(IDLE_AFTER_MESSAGE_S);
    tick(&rig);
    assert!(lock(&rig.host.releases).is_empty());
    tick(&rig);
    assert_eq!(fake.sent().len(), 1);
    assert_eq!(rig.state_of("cli-claude-00000001"), "stalled");

    *lock(&rig.host.fail_release) = false;
    rig.clock.advance(SILENT_S);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1, "released without a second reminder");
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("cli-claude-00000001"), ticket_id("T.1"))]
    );
}

/// A rig whose one worker holds `T.1`, waits on a background `ignite test` that started now, and
/// has just started the test's step `step`, or reported none.
fn waiting_on_test(command: &str, step: Option<&str>) -> (super::tests::Rig, Arc<FakeCliPool>) {
    let rig = rig(&["cli-claude-00000001"]);
    let fake = cli(&rig, 1);
    rig.hold("cli-claude-00000001", "T.1");
    let now = rig.clock.now();
    lock(&rig.host.commands).insert(
        session("cli-claude-00000001"),
        SessionCommand {
            command: command.to_owned(),
            since: now,
            running: true,
        },
    );
    let verb = |name: &str| RunningVerb {
        verb: name.to_owned(),
        handle: None,
        started: now,
        step: step.map(|s| (s.to_owned(), now)),
    };
    let mut verbs = vec![verb("test")];
    if command.starts_with("land") {
        verbs.push(verb("land"));
    }
    lock(&rig.host.verbs).insert(session("cli-claude-00000001"), verbs);
    tick(&rig);
    (rig, fake)
}

#[test]
fn an_idle_worker_with_a_stuck_step_gets_one_message_and_is_released_15_minutes_later() {
    let (rig, fake) = waiting_on_test("wait L-1", Some("listing family b3d-interface"));
    rig.clock.advance(STUCK_STEP_S - 60.0);
    tick(&rig);
    assert!(fake.sent().is_empty());
    assert!(lock(&rig.host.releases).is_empty());
    rig.clock.advance(60.0);
    fake.set_check(&session("cli-claude-00000001"), SessionCheck::Idle);
    tick(&rig);
    let sent = fake.sent();
    let prompt = &sent[0].text;
    for part in [
        "`ignite test` has run for 30 min",
        "\"listing family b3d-interface\" for 30 min",
        "make it a dependency of this one",
        "ignite justiciar",
    ] {
        assert!(prompt.contains(part), "{part} missing from: {prompt}");
    }
    rig.clock.advance(IDLE_AFTER_MESSAGE_S - 60.0);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1);
    assert!(lock(&rig.host.releases).is_empty());
    rig.clock.advance(60.0);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1);
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("cli-claude-00000001"), ticket_id("T.1"))]
    );
}

#[test]
fn a_finished_command_gives_the_resumed_worker_a_new_silence_period() {
    let (rig, fake) = waiting_on_test("check", None);
    let worker = session("cli-claude-00000001");
    fake.set_check(&worker, SessionCheck::WaitingForVerb);
    rig.clock.advance(STUCK_COMMAND_S);
    tick(&rig);
    assert!(fake.sent().is_empty());

    lock(&rig.host.verbs).remove(&worker);
    let previous = lock(&rig.host.commands).remove(&worker).unwrap();
    lock(&rig.host.quiet).insert(worker.clone(), previous.since);
    fake.set_check(&worker, SessionCheck::Idle);
    rig.clock.advance(IDLE_AFTER_MESSAGE_S);
    tick(&rig);
    assert!(fake.sent().is_empty());
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_command_without_steps_is_stuck_after_45_minutes_and_a_running_worker_is_left_alone() {
    let (rig, fake) = waiting_on_test("wait L-1", None);
    rig.clock.advance(STUCK_COMMAND_S - 60.0);
    tick(&rig);
    assert!(fake.sent().is_empty());
    assert!(lock(&rig.host.releases).is_empty());
    rig.clock.advance(60.0);
    fake.set_check(&session("cli-claude-00000001"), SessionCheck::Running);
    tick(&rig);
    rig.clock.advance(60.0);
    tick(&rig);
    assert!(fake.sent().is_empty());
    assert!(lock(&rig.host.releases).is_empty());
    rig.clock.advance(SILENT_S - 60.0);
    fake.set_check(&session("cli-claude-00000001"), SessionCheck::Idle);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1);
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_worker_that_is_landing_is_never_checked_for_a_stuck_command() {
    let (rig, fake) = waiting_on_test("land", Some("waiting for tip verification"));
    rig.clock.advance(2.0 * STUCK_COMMAND_S);
    tick(&rig);
    assert!(fake.sent().is_empty());
    assert!(lock(&rig.host.releases).is_empty());
}
