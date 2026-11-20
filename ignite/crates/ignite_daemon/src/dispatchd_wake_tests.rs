//! Workers that end their turn waiting on an `ignite` verb are let stop and woken, and workers
//! whose ticket Ignite released may not claim again until the dispatcher hands them a ticket.

use ignite_core::traits::Clock;

use super::restart_tests::DbFile;
use super::tests::{Rig, START, detail, lock, request, rig, rig_on, session, ticket_id};
use super::wake::{WAKE, WAKE_S};
use super::*;
use crate::server::Verb;

/// A rig whose one pool worker, `local_w1`, holds `T.1`.
fn holding() -> Rig {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    assert!(rig.step().is_none());
    rig
}

fn ended(rig: &Rig, args: &[&str]) -> String {
    let verb = TurnEndedVerb::new(rig.dispatcher.clone(), "jarvis".to_owned());
    let reply = verb.run(&rig.ctx, &request("turn-ended", args, None));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    reply.text
}

fn verdict(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or("")
}

/// `local_w1` started `ignite <verb>` in the background now, and is no longer waiting on it.
fn runs_in_background(rig: &Rig, verb: &str) {
    lock(&rig.host.verbs).insert(
        session("local_w1"),
        vec![RunningVerb {
            verb: verb.to_owned(),
            started: rig.clock.now(),
            step: None,
        }],
    );
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: format!("{verb} x"),
            since: rig.clock.now(),
            running: false,
        },
    );
}

/// The background verb of `local_w1` finished now.
fn finishes(rig: &Rig) {
    lock(&rig.host.verbs).remove(&session("local_w1"));
    lock(&rig.host.commands)
        .get_mut(&session("local_w1"))
        .unwrap()
        .since = rig.clock.now();
}

#[test]
fn a_worker_waiting_on_a_background_run_is_let_stop_and_never_released() {
    let rig = holding();
    runs_in_background(&rig, "run");
    let first = ended(&rig, &["--session", "local_w1"]);
    assert_eq!(verdict(&first), "pass", "{first}");
    assert!(first.contains("`ignite run` is still running"), "{first}");
    let again = ended(&rig, &["--session", "local_w1", "--again"]);
    assert_eq!(verdict(&again), "pass", "{again}");
    assert!(lock(&rig.host.releases).is_empty());
    // While the run goes on, nothing checks the worker.
    rig.clock.advance(WAKE_S * 5.0);
    assert!(rig.step().is_none());
}

#[test]
fn a_waiting_worker_nothing_woke_is_told_its_command_finished() {
    let rig = holding();
    runs_in_background(&rig, "test");
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
    rig.clock.advance(600.0);
    finishes(&rig);
    assert!(rig.step().is_none(), "checked before its tool could wake it");
    rig.clock.advance(WAKE_S);
    let check = rig.step().unwrap();
    assert_eq!(check.action, DispatchAction::Check);
    rig.ack(check.id, "idle", "");
    let wake = rig.step().unwrap();
    assert_eq!(wake.action, DispatchAction::Message);
    assert_eq!(wake.prompt.as_deref(), Some(WAKE));
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_waiting_worker_its_tool_woke_gets_no_message() {
    let rig = holding();
    runs_in_background(&rig, "check");
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
    finishes(&rig);
    rig.clock.advance(WAKE_S);
    let check = rig.step().unwrap();
    rig.ack(check.id, "running", "");
    rig.clock.advance(60.0);
    assert!(rig.step().is_none());
}

#[test]
fn the_block_text_names_the_way_to_wait() {
    let rig = holding();
    let text = ended(&rig, &["--session", "local_w1"]);
    assert_eq!(verdict(&text), "block", "{text}");
    assert!(text.contains("`ignite run -- <command>`"), "{text}");
    assert!(text.contains("wakes you when it finishes"), "{text}");
}

#[test]
fn a_worker_released_at_turn_end_may_not_claim_again() {
    let rig = holding();
    let text = ended(&rig, &["--session", "local_w1", "--again"]);
    assert_eq!(verdict(&text), "released", "{text}");
    assert!(text.contains("Stop now"), "{text}");
    let w1 = session("local_w1");
    let t1 = ticket_id("T.1");
    let refusal = rig.dispatcher.refuse_start(&w1, Some(&t1)).unwrap();
    assert!(refusal.starts_with("refused"), "{refusal}");
    assert!(refusal.contains("end your turn"), "{refusal}");
    assert!(rig.dispatcher.refuse_start(&w1, Some(&ticket_id("T.5"))).is_some());
    assert!(rig.dispatcher.refuse_start(&w1, None).is_some());
    // Another session is not affected.
    assert!(
        rig.dispatcher
            .refuse_start(&session("local_w2"), Some(&t1))
            .is_none()
    );
}

#[test]
fn a_released_worker_may_start_only_the_ticket_handed_to_it() {
    let rig = holding();
    ended(&rig, &["--session", "local_w1", "--again"]);
    assert!(rig.step().is_none());
    rig.clock.advance(SETTLE_S);
    rig.reserve("T.2", "local_w1", &[]);
    let handoff = rig.step().unwrap();
    assert_eq!(handoff.action, DispatchAction::Handoff);
    assert!(handoff.clear, "a hand-off clears the session first");
    let w1 = session("local_w1");
    assert!(rig.dispatcher.refuse_start(&w1, Some(&ticket_id("T.2"))).is_none());
    assert!(rig.dispatcher.refuse_start(&w1, Some(&ticket_id("T.1"))).is_some());
    // Once it holds the handed ticket, the record ends.
    rig.ack(handoff.id, "ok", "");
    rig.hold("local_w1", "T.2");
    rig.step();
    assert!(rig.dispatcher.refuse_start(&w1, Some(&ticket_id("T.7"))).is_none());
}

#[test]
fn a_worker_released_by_the_stall_rule_may_not_claim_again() {
    let rig = holding();
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "brief".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    rig.clock.advance(SILENT_S);
    let check = rig.step().unwrap();
    rig.ack(check.id, "idle", "");
    let reminder = rig.step().unwrap();
    rig.ack(reminder.id, "ok", "");
    rig.clock.advance(IDLE_AFTER_MESSAGE_S);
    let recheck = rig.step().unwrap();
    rig.ack(recheck.id, "idle", "");
    assert_eq!(lock(&rig.host.releases).len(), 1);
    let refusal = rig
        .dispatcher
        .refuse_start(&session("local_w1"), Some(&ticket_id("T.1")));
    assert!(refusal.is_some());
}

#[test]
fn a_restarted_daemon_still_refuses_a_released_worker() {
    let file = DbFile::new("released");
    let now = Timestamp::from_millis(START);
    let first = rig_on(&["local_w1"], file.open(), now);
    first.hold("local_w1", "T.1");
    first
        .hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    first.step();
    ended(&first, &["--session", "local_w1", "--again"]);
    let restarted = rig_on(&[], file.open(), now);
    let refusal = restarted
        .dispatcher
        .refuse_start(&session("local_w1"), Some(&ticket_id("T.1")));
    assert!(refusal.is_some());
}
