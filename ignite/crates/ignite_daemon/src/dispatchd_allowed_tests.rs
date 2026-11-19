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
    for tool in [CLEAR, MODEL, EFFORT] {
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
    // The prompt waits until the model and effort read back as asked.
    assert!(!allows(&rig, SEND, Some("local_w1")), "no read-back yet");
    let readback = format!(
        r#"{{"sessionId":"local_w1","model":"{}","effort":"{}"}}"#,
        handoff.model.as_deref().unwrap_or_default(),
        handoff.effort.as_deref().unwrap_or_default()
    );
    rig.ack(handoff.id, "raw", &readback);
    assert!(allows(&rig, SEND, Some("local_w1")), "read back as asked");
    assert!(!allows(&rig, SEND, Some("local_w2")));
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

#[test]
fn once_clears_are_reported_a_refused_clear_stops_the_handoff() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.reserve("T.1", "local_w1", &[]);
    let first = rig.step().unwrap();
    assert!(first.clear);
    assert!(
        allows(&rig, MODEL, Some("local_w1")),
        "a dispatcher whose hook never reported a clear is not held to it"
    );
    // Its hook reports the clear, and from then on every hand-off waits for one.
    let cleared = DispatchVerb::new(rig.dispatcher.clone()).run(
        &rig.ctx,
        &request("dispatch", &["cleared", "local_w1"], None),
    );
    assert_eq!(cleared.exit, Exit::Success, "{}", cleared.text);
    assert!(allows(&rig, MODEL, Some("local_w1")));
    assert!(allows(&rig, EFFORT, Some("local_w1")));
    rig.ack(first.id, "ok", "");

    // The app refuses the next clear, so no hook reports it.
    rig.reserve("T.2", "local_w2", &[]);
    let second = rig.step().unwrap();
    assert_eq!(handoff_to(&second).0, "local_w2");
    assert!(allows(&rig, CLEAR, Some("local_w2")));
    for tool in [MODEL, EFFORT] {
        assert!(
            !allows(&rig, tool, Some("local_w2")),
            "{tool} on a busy worker"
        );
    }
    // A read-back that happens to match still fails the hand-off.
    let readback = format!(
        r#"{{"sessionId":"local_w2","model":"{}","effort":"{}"}}"#,
        second.model.as_deref().unwrap_or_default(),
        second.effort.as_deref().unwrap_or_default()
    );
    let raw = DispatchVerb::new(rig.dispatcher.clone()).run(
        &rig.ctx,
        &request(
            "dispatch",
            &["ack", &second.id.to_string(), "raw", &readback],
            None,
        ),
    );
    assert!(raw.text.contains("Do not send the prompt"), "{}", raw.text);
    assert!(!allows(&rig, SEND, Some("local_w2")));
}
