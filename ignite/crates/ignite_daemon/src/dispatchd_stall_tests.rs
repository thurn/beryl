//! How often a silent worker, or one whose command is stuck, is checked: once per silence or
//! stuck command, whatever the answer.

use super::stall::{STUCK_COMMAND_S, STUCK_STEP_S};
use super::tests::{lock, rig, session, ticket_id};
use super::*;
use ignite_core::traits::Clock;

/// A rig whose one worker holds `T.1` and last ran a command at the start.
fn silent_worker() -> super::tests::Rig {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "test x".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    assert!(rig.step().is_none());
    rig.clock.advance(SILENT_S);
    rig
}

#[test]
fn a_worker_answered_running_is_checked_again_only_after_another_silence() {
    let rig = silent_worker();
    let check = rig.step().unwrap();
    assert_eq!(check.action, DispatchAction::Check);
    rig.ack(check.id, "running", "");
    // The silence has not changed, but the worker was just seen running.
    rig.clock.advance(30.0);
    assert!(rig.step().is_none(), "checked again 30 seconds later");
    rig.clock.advance(SILENT_S - 30.0);
    assert_eq!(rig.step().unwrap().action, DispatchAction::Check);
}

#[test]
fn a_release_that_fails_is_retried_at_the_next_check_without_another_reminder() {
    let rig = silent_worker();
    *lock(&rig.host.fail_release) = true;
    let check = rig.step().unwrap();
    rig.ack(check.id, "idle", "");
    let reminder = rig.step().unwrap();
    assert_eq!(reminder.action, DispatchAction::Message);
    rig.ack(reminder.id, "ok", "");
    rig.clock.advance(IDLE_AFTER_MESSAGE_S);
    let recheck = rig.step().unwrap();
    let reply = rig.ack(recheck.id, "idle", "");
    assert!(reply.text.contains("could not release"), "{}", reply.text);
    assert!(rig.step().is_none(), "no check or reminder right after");
    assert_eq!(rig.state_of("local_w1"), "stalled");

    *lock(&rig.host.fail_release) = false;
    rig.clock.advance(SILENT_S);
    let retry = rig.step().unwrap();
    assert_eq!(retry.action, DispatchAction::Check);
    rig.ack(retry.id, "idle", "");
    assert!(rig.step().is_none(), "released without a second reminder");
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
}

#[test]
fn the_check_asks_for_get_sessions_result_and_tells_codex_an_archived_session_is_gone() {
    let rig = silent_worker();
    let check = rig.step().unwrap();
    let prompt = check.prompt.unwrap();
    assert!(prompt.contains("Call get_session"), "{prompt}");
    assert!(prompt.contains("an archived session is `gone`"), "{prompt}");
}

/// A rig whose one worker holds `T.1`, waits on a background `ignite test` that started now, and
/// has just started the test's step `step`, or reported none.
fn waiting_on_test(command: &str, step: Option<&str>) -> super::tests::Rig {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    let now = rig.clock.now();
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: command.to_owned(),
            since: now,
            running: true,
        },
    );
    let verb = |name: &str| RunningVerb {
        verb: name.to_owned(),
        started: now,
        step: step.map(|s| (s.to_owned(), now)),
    };
    let mut verbs = vec![verb("test")];
    if command.starts_with("land") {
        verbs.push(verb("land"));
    }
    lock(&rig.host.verbs).insert(session("local_w1"), verbs);
    assert!(rig.step().is_none());
    rig
}

#[test]
fn an_idle_worker_with_a_stuck_step_gets_one_message_and_is_released_15_minutes_later() {
    let rig = waiting_on_test("wait L-1", Some("listing family b3d-interface"));
    rig.clock.advance(STUCK_STEP_S - 60.0);
    assert!(
        rig.step().is_none(),
        "checked before the step reached 30 minutes"
    );
    rig.clock.advance(60.0);
    let check = rig.step().unwrap();
    assert_eq!(check.action, DispatchAction::Check);
    rig.ack(check.id, "idle", "");
    let message = rig.step().unwrap();
    assert_eq!(message.action, DispatchAction::Message);
    let prompt = message.prompt.unwrap();
    for part in [
        "`ignite test` has run for 30 min",
        "\"listing family b3d-interface\" for 30 min",
        "make it a dependency of this one",
        "ignite justiciar",
    ] {
        assert!(prompt.contains(part), "{part} missing from: {prompt}");
    }
    rig.ack(message.id, "ok", "");
    rig.clock.advance(IDLE_AFTER_MESSAGE_S - 60.0);
    assert!(rig.step().is_none(), "checked again before 15 minutes");
    rig.clock.advance(60.0);
    let recheck = rig.step().unwrap();
    assert_eq!(recheck.action, DispatchAction::Check);
    let reply = rig.ack(recheck.id, "idle", "");
    assert!(
        reply.text.contains("command stuck past its time"),
        "{}",
        reply.text
    );
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
}

#[test]
fn a_command_without_steps_is_stuck_after_45_minutes_and_a_running_worker_is_left_alone() {
    let rig = waiting_on_test("wait L-1", None);
    rig.clock.advance(STUCK_COMMAND_S - 60.0);
    assert!(rig.step().is_none());
    rig.clock.advance(60.0);
    let check = rig.step().unwrap();
    assert_eq!(check.action, DispatchAction::Check);
    rig.ack(check.id, "running", "");
    rig.clock.advance(60.0);
    assert!(
        rig.step().is_none(),
        "a running worker was checked again a minute later"
    );
    rig.clock.advance(SILENT_S - 60.0);
    assert_eq!(rig.step().unwrap().action, DispatchAction::Check);
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_worker_that_is_landing_is_never_checked_for_a_stuck_command() {
    let rig = waiting_on_test("land", Some("waiting for tip verification"));
    rig.clock.advance(2.0 * STUCK_COMMAND_S);
    assert!(rig.step().is_none());
}
