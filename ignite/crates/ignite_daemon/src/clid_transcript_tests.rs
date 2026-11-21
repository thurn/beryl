//! Tests of the digest of recorded Claude and Codex streams, and of sending a turn's digest to the
//! hub through the outbox, including after a restart and as soon as the lines are queued.

use ignite_core::fakes::FakeHub;
use ignite_core::protocol::cli::{DIGEST_LINE_CHARS, TranscriptKind};

use super::*;
use crate::clid::transcript::digest;

const CLAUDE: &str = include_str!("../../ignite_core/src/cli/fixtures/first_turn.jsonl");
const CODEX: &str = include_str!("../../ignite_core/src/cli/codex_fixtures/clean_with_tool.jsonl");

fn kinds(lines: &[TranscriptLine]) -> Vec<(u32, TranscriptKind, &str)> {
    lines
        .iter()
        .map(|l| (l.index, l.kind, l.text.as_str()))
        .collect()
}

#[test]
fn a_recorded_claude_turn_is_digested_with_its_place_in_the_session() {
    let session = SessionId::new("cli-claude-0a1b2c3d").unwrap();
    let lines = digest("claude", CLAUDE, &session, Some(&ticket()), 2);
    assert_eq!(
        kinds(&lines),
        [
            (0, TranscriptKind::Assistant, "done"),
            (1, TranscriptKind::Result, "success: done"),
        ]
    );
    assert!(lines.iter().all(|l| l.session == session && l.turn == 2));
    assert!(lines.iter().all(|l| l.ticket.as_ref() == Some(&ticket())));
}

#[test]
fn a_recorded_codex_turn_is_digested() {
    let session = SessionId::new("cli-codex-0a1b2c3d").unwrap();
    let lines = digest("codex", CODEX, &session, None, 1);
    assert_eq!(
        kinds(&lines),
        [
            (0, TranscriptKind::Assistant, "I’ll run `echo hi`."),
            (
                1,
                TranscriptKind::Tool,
                "command_execution: /bin/zsh -lc 'echo hi'"
            ),
            (2, TranscriptKind::ToolResult, "hi"),
            (3, TranscriptKind::Assistant, "done"),
            (4, TranscriptKind::Result, "completed"),
        ]
    );
    assert_eq!(
        lines[2].call,
        Some(1),
        "the output names its command's line"
    );
    assert!(
        lines
            .iter()
            .filter(|l| l.index != 2)
            .all(|l| l.call.is_none())
    );
}

#[test]
fn a_long_line_is_cut_and_a_half_written_line_waits() {
    let session = SessionId::new("cli-claude-0a1b2c3d").unwrap();
    let long = "x".repeat(DIGEST_LINE_CHARS + 500);
    let event = format!(
        r#"{{"type":"assistant","parent_tool_use_id":null,"message":{{"content":[{{"type":"text","text":"{long}"}}]}}}}"#
    );
    let lines = digest(
        "claude",
        &format!("{event}\n{{\"type\":\"res"),
        &session,
        None,
        1,
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text.chars().count(), DIGEST_LINE_CHARS);
    assert!(lines[0].text.ends_with('…'));
}

/// Moves what the outbox holds to `hub`.
fn flush(rig: &Rig, hub: &FakeHub) {
    rig.pool
        .with_database(|conn| crate::outbox::flush(conn, hub))
        .unwrap();
}

#[test]
fn a_restart_sends_the_digest_again_and_the_hub_keeps_each_line_once() {
    let mut rig = Rig::new();
    rig.daemon = rig.daemon.with_machine(MachineId::new("m-1").unwrap());
    let hub = FakeHub::default();
    let session = SessionId::new("cli-claude-0a1b2c3d").unwrap();
    let first_line = CLAUDE.lines().take(2).collect::<Vec<_>>().join("\n") + "\n";

    // The turn is read while it runs, then again after it ended.
    rig.daemon
        .send_digest("claude", &session, Some(&ticket()), 1, &first_line);
    rig.daemon
        .send_digest("claude", &session, Some(&ticket()), 1, CLAUDE);
    // Looking again at an unchanged turn queues nothing.
    rig.daemon
        .send_digest("claude", &session, Some(&ticket()), 1, CLAUDE);
    flush(&rig, &hub);
    assert_eq!(hub.transcript_lines().len(), 2);

    // A restarted daemon has forgotten what it queued and queues the whole turn once more.
    lock(&rig.daemon.sent).clear();
    rig.daemon
        .send_digest("claude", &session, Some(&ticket()), 1, CLAUDE);
    flush(&rig, &hub);
    let held = hub.transcript_lines();
    assert_eq!(
        held.iter().map(|l| l.index).collect::<Vec<_>>(),
        [0, 1],
        "each line is held once"
    );
    assert!(held.iter().all(|l| l.at.is_some()));
}

#[test]
fn the_digest_carries_the_model_and_effort_of_the_session_slot() {
    let mut rig = Rig::new();
    rig.daemon = rig.daemon.with_machine(MachineId::new("m-1").unwrap());
    let hub = FakeHub::default();
    let session = SessionId::new("cli-claude-0a1b2c3d").unwrap();
    rig.pool
        .with_database(|conn| {
            store::put(
                conn,
                &CliSlot {
                    app: "claude".to_owned(),
                    slot: 1,
                    session: Some(session.to_string()),
                    ticket: Some(ticket().to_string()),
                    state: "running".to_owned(),
                    turn: 1,
                    transcript: None,
                    model: Some("claude-opus-5-5".to_owned()),
                    effort: Some("high".to_owned()),
                    since: None,
                    reservation: None,
                    next_prompt: None,
                    wait_handle: None,
                    resume_at: None,
                    park_reason: None,
                    interrupted_in_row: 0,
                    block_sent: 0,
                    backed_off: 0,
                    fallback_prompt: None,
                },
            )
        })
        .unwrap();
    rig.daemon
        .send_digest("claude", &session, Some(&ticket()), 1, CLAUDE);
    flush(&rig, &hub);
    let sessions = hub.transcript_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session, session);
    assert_eq!(sessions[0].model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(sessions[0].effort.as_deref(), Some("high"));
}

#[test]
fn a_hub_outage_loses_no_line() {
    let mut rig = Rig::new();
    rig.daemon = rig.daemon.with_machine(MachineId::new("m-1").unwrap());
    let hub = FakeHub::default();
    let session = SessionId::new("cli-claude-0a1b2c3d").unwrap();
    rig.daemon.send_digest("claude", &session, None, 1, CLAUDE);
    hub.set_unreachable(true);
    assert!(
        rig.pool
            .with_database(|conn| crate::outbox::flush(conn, &hub))
            .is_err()
    );
    assert!(hub.transcript_lines().is_empty());
    hub.set_unreachable(false);
    flush(&rig, &hub);
    assert_eq!(hub.transcript_lines().len(), 2);
}

#[test]
fn a_session_digest_reads_every_turn_from_its_log() {
    let rig = Rig::new();
    rig.keep_running(1);
    let session = rig.begin();
    // The fake app's streams are not any real app's, so the digest of this session is empty, but
    // the session and its turns are found by ticket as well as by session.
    let (found, lines) = rig.daemon.session_digest("T.1").unwrap().unwrap();
    assert_eq!(found, session);
    assert!(lines.is_empty());
    assert!(rig.daemon.session_digest("T.9").unwrap().is_none());
}

#[test]
fn queueing_lines_wakes_the_sender_and_a_tool_result_reaches_the_hub_with_its_call() {
    let mut rig = Rig::new();
    rig.daemon = rig.daemon.with_machine(MachineId::new("m-1").unwrap());
    let hub = FakeHub::default();
    let session = SessionId::new("cli-codex-0a1b2c3d").unwrap();
    rig.daemon.send_digest("codex", &session, None, 1, CODEX);
    // The sender's pause between rounds ends at once: lines are waiting.
    assert!(crate::outbox::wait_for_transcripts(
        std::time::Duration::ZERO
    ));
    flush(&rig, &hub);
    let held = hub.transcript_lines();
    let result = held
        .iter()
        .find(|l| l.kind == TranscriptKind::ToolResult)
        .unwrap();
    assert_eq!((result.text.as_str(), result.call), ("hi", Some(1)));
}
