//! `ignite dispatch allowed`: which session tool calls the current instruction asks for.

use super::allowed::{ROTATE_S, SETUP_S};
use super::tests::{Rig, handoff_to, request, rig, session};
use super::*;
use crate::server::Verb;
use ignite_core::traits::Clock;

const CLEAR: &str = "mcp__ccd_session_mgmt__clear_session";
const MODEL: &str = "mcp__ccd_session_mgmt__set_session_model";
const EFFORT: &str = "mcp__ccd_session_mgmt__set_session_effort";
const SEND: &str = "mcp__ccd_session_mgmt__send_message";
const START: &str = "mcp__ccd_session__start_session";

fn allows(rig: &Rig, tool: &str, target: Option<&str>) -> bool {
    let mut args = vec!["allowed", tool];
    args.extend(target);
    let reply =
        DispatchVerb::new(rig.dispatcher.clone()).run(&rig.ctx, &request("dispatch", &args, None));
    reply.exit == Exit::Success
}

#[test]
fn a_handoff_allows_its_own_calls_on_its_own_worker_until_acknowledged() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.pool(
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"],
        None,
    );
    rig.pool(
        &["add", "local_w1", "local_w2", "--app", "claude"],
        Some("local_dispatcher"),
    );
    assert!(!allows(&rig, CLEAR, Some("local_w1")), "nothing given yet");
    rig.reserve("T.1", "local_w1", &[]);
    let handoff = rig.step().unwrap();
    assert_eq!(handoff_to(&handoff).0, "local_w1");
    for tool in [CLEAR, MODEL, EFFORT, SEND] {
        assert!(allows(&rig, tool, Some("local_w1")), "{tool}");
        assert!(
            !allows(&rig, tool, Some("local_w2")),
            "{tool} on another worker"
        );
    }
    assert!(!allows(&rig, CLEAR, Some("self")));
    assert!(
        !allows(&rig, START, None),
        "a hand-off to a named worker starts nothing"
    );
    assert!(!allows(
        &rig,
        "mcp__ccd_session_mgmt__archive_session",
        Some("local_w1")
    ));
    // Given again after two minutes unacknowledged, it may still be under way.
    rig.clock.advance(ACK_WAIT_S + 1.0);
    assert!(rig.step().is_some());
    assert!(allows(&rig, CLEAR, Some("local_w1")));
    rig.ack(handoff.id, "ok", "");
    assert!(!allows(&rig, SEND, Some("local_w1")));
}

#[test]
fn a_message_allows_only_sending_to_its_session() {
    let rig = rig(&["local_w1"]);
    Dispatcher::queue(
        &mut rig.dispatcher.state(),
        Some(&session("local_w1")),
        DispatchAction::Message,
        "Continue.",
        Purpose::Message,
    );
    let message = rig.step().unwrap();
    assert_eq!(message.action, DispatchAction::Message);
    assert!(allows(&rig, SEND, Some("local_w1")));
    assert!(!allows(&rig, CLEAR, Some("local_w1")));
    assert!(!allows(&rig, MODEL, Some("local_w1")));
}

#[test]
fn the_pool_setup_may_start_workers_until_they_are_registered() {
    let rig = rig(&[]);
    assert!(allows(&rig, START, None), "no dispatcher is registered yet");
    rig.pool(
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"],
        None,
    );
    assert!(allows(&rig, START, None), "the setup is in progress");
    rig.pool(
        &["add", "local_w1", "--app", "claude"],
        Some("local_dispatcher"),
    );
    assert!(!allows(&rig, START, None), "the workers are registered");
    rig.pool(
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"],
        None,
    );
    rig.clock.advance(SETUP_S);
    assert!(
        !allows(&rig, START, None),
        "a setup that never finished lapses"
    );
}

#[test]
fn a_rotate_allows_starting_the_next_dispatcher_for_a_while() {
    let rig = rig(&[]);
    rig.pool(
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"],
        None,
    );
    rig.dispatcher.pool_setup(false, rig.clock.now());
    rig.pool(
        &["add", "local_codex", "--app", "codex", "--dispatcher"],
        None,
    );
    rig.dispatcher.pool_setup(false, rig.clock.now());
    rig.dispatcher
        .state()
        .given_to
        .insert(session("local_codex"), ROTATE_AFTER);
    assert!(!allows(&rig, START, None));
    let rotate = rig
        .dispatcher
        .step(&rig.ctx, Some(&session("local_codex")))
        .unwrap();
    assert_eq!(rotate.action, DispatchAction::Rotate);
    assert!(allows(&rig, START, None));
    rig.clock.advance(ROTATE_S);
    assert!(!allows(&rig, START, None));
}

#[test]
fn a_refusal_names_the_current_instruction() {
    let rig = rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &[]);
    rig.step().unwrap();
    let reply = rig.dispatcher.allowed(&rig.ctx, CLEAR, Some("local_w9"));
    assert_eq!(reply.exit, Exit::CheckFailed);
    assert!(reply.text.contains("local_w9"), "{}", reply.text);
    assert!(reply.text.contains(r#""do":"handoff""#), "{}", reply.text);
}
