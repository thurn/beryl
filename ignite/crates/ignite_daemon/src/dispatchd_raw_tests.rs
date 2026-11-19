//! A Claude dispatcher reports `get_session`'s raw result, and the daemon decides what it means.

use ignite_core::protocol::client::{DispatchAction, Exit};
use ignite_core::traits::Clock;
use serde_json::json;

use super::raw::{check_answer, readback_mismatch};
use super::tests::{Rig, handoff_to, lock, rig, session};
use super::*;

/// What `get_session` returns for the worker of `instruction` once its model and effort stuck.
fn readback(instruction: &DispatchInstruction) -> String {
    json!({
        "sessionId": instruction.session.as_ref().map(SessionId::as_str),
        "isArchived": false,
        "isRunning": false,
        "model": instruction.model,
        "effort": instruction.effort,
    })
    .to_string()
}

impl Rig {
    /// Reports a hand-off carried out: a read-back that matches, then `ok`.
    pub(super) fn handed(&self, instruction: &DispatchInstruction) -> Reply {
        let raw = self.ack(instruction.id, "raw", &readback(instruction));
        assert_eq!(raw.exit, Exit::Success, "{}", raw.text);
        self.ack(instruction.id, "ok", "")
    }

    /// A worker that holds a ticket, has run nothing for 30 minutes and is due a check.
    fn silent(&self, worker: &str) -> DispatchInstruction {
        self.hold(worker, "T.1");
        lock(&self.host.commands).insert(
            session(worker),
            SessionCommand {
                command: "test x".to_owned(),
                since: self.clock.now(),
                running: false,
            },
        );
        self.clock.advance(SILENT_S);
        let check = self.step().unwrap();
        assert_eq!(check.action, DispatchAction::Check);
        check
    }
}

#[test]
fn a_check_reads_the_session_from_get_sessions_result() {
    let w1 = session("local_w1");
    let raw = |fields: serde_json::Value| fields.to_string();
    let cases = [
        (raw(json!({"sessionId": "local_w1", "isRunning": true})), "running"),
        (
            raw(json!({"sessionId": "local_w1", "isRunning": false, "isArchived": false})),
            "idle",
        ),
        (
            raw(json!({"sessionId": "local_w1", "isRunning": true, "isArchived": true})),
            "gone",
        ),
        ("Session local_w1 not found.".to_owned(), "gone"),
        ("Request timed out".to_owned(), "unknown"),
        ("Tool get_session not found".to_owned(), "unknown"),
        (raw(json!({"sessionId": "local_w2", "isRunning": true})), "unknown"),
        (raw(json!({"sessionId": "local_w1"})), "unknown"),
        ("[]".to_owned(), "unknown"),
    ];
    for (result, answer) in cases {
        assert_eq!(check_answer(&w1, &result), answer, "{result}");
    }
}

#[test]
fn a_raw_gone_releases_the_ticket_and_a_raw_error_does_not() {
    let rig = rig(&["local_w1"]);
    let check = rig.silent("local_w1");
    let reply = rig.ack(check.id, "raw", "Request timed out");
    assert!(reply.text.starts_with("read local_w1 as unknown"), "{}", reply.text);
    assert!(lock(&rig.host.releases).is_empty());

    let rig = super::tests::rig(&["local_w1"]);
    let check = rig.silent("local_w1");
    rig.ack(check.id, "raw", "Session local_w1 not found.");
    assert_eq!(rig.state_of("local_w1"), "down");
    assert_eq!(lock(&rig.host.releases).len(), 1);
}

#[test]
fn a_handoff_is_ok_only_after_a_read_back_that_matches() {
    let rig = rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &["tier:opus"]);
    let handoff = rig.step().unwrap();
    let refused = rig.ack(handoff.id, "ok", "");
    assert_eq!(refused.exit, Exit::Usage, "{}", refused.text);
    assert!(refused.text.contains("raw"), "{}", refused.text);

    let matched = rig.ack(handoff.id, "raw", &readback(&handoff));
    assert!(matched.text.contains("send `prompt`"), "{}", matched.text);
    rig.clock.advance(ACK_WAIT_S - 1.0);
    assert!(rig.step().is_none(), "the read-back restarted the wait");
    assert_eq!(rig.ack(handoff.id, "ok", "").exit, Exit::Success);
    assert_eq!(rig.state_of("local_w1"), "reserved");
}

#[test]
fn a_read_back_on_another_model_or_effort_fails_the_handoff() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.reserve("T.1", "local_w1", &["tier:opus"]);
    let handoff = rig.step().unwrap();
    let mut wrong = handoff.clone();
    wrong.model = Some("sonnet".to_owned());
    let reply = rig.ack(handoff.id, "raw", &readback(&wrong));
    assert!(
        reply.text.contains("model read back as sonnet, not opus"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("Do not send the prompt"), "{}", reply.text);
    let moved = rig.step().unwrap();
    assert_eq!(handoff_to(&moved), ("local_w2", "T.1"));
    assert_eq!(rig.ack(handoff.id, "ok", "").exit, Exit::Usage, "already failed");
}

#[test]
fn a_read_back_compares_model_without_its_window_and_effort_exactly() {
    let mut body = DispatchInstruction {
        id: 1,
        action: DispatchAction::Handoff,
        session: Some(session("local_w1")),
        clear: true,
        model: Some("claude-opus-5-5".to_owned()),
        effort: Some("high".to_owned()),
        prompt: None,
    };
    let mut got = body.clone();
    got.model = Some("claude-opus-5-5[1m]".to_owned());
    assert_eq!(readback_mismatch(&body, &readback(&got)), None);
    got.effort = Some("medium".to_owned());
    assert_eq!(
        readback_mismatch(&body, &readback(&got)).as_deref(),
        Some("effort read back as medium, not high")
    );
    assert!(readback_mismatch(&body, "Session local_w1 not found.").is_some());
    got = body.clone();
    got.session = Some(session("local_w2"));
    assert!(readback_mismatch(&body, &readback(&got)).is_some());
    // A hand-off that names no model (no `[pool.models]` entry) checks only the effort.
    body.model = None;
    got = body.clone();
    got.model = Some("claude-sonnet-5-5".to_owned());
    assert_eq!(readback_mismatch(&body, &readback(&got)), None);
}
