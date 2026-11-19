//! How often a silent worker is checked: once per silence, whatever the answer.

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
