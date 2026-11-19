"""The `Stop` hook names the session Ignite knows, and relays only Ignite's `block`."""

import json
from pathlib import Path

from beryl_tools import turn_ended

DESKTOP = {"CLAUDE_CODE_HOST_SESSION_ID": "local_w1"}


def test_the_session_is_the_one_ignite_records() -> None:
    assert turn_ended.session(DESKTOP) == "local_w1"
    assert (
        turn_ended.session({"IGNITE_SESSION": "local_x", **DESKTOP}) == "local_x"
    )
    assert turn_ended.session({}) is None


def test_a_first_stop_asks_without_a_note() -> None:
    event = {"session_id": "cli-uuid", "stop_hook_active": False}
    assert turn_ended.command(event, DESKTOP) == [
        "ignite",
        "turn-ended",
        "--session",
        "local_w1",
    ]


def test_a_second_stop_sends_the_last_message_as_the_note() -> None:
    event = {"stop_hook_active": True, "last_assistant_message": "  Blocked on T.9.  "}
    assert turn_ended.command(event, DESKTOP) == [
        "ignite",
        "turn-ended",
        "--session",
        "local_w1",
        "--again",
        "--note",
        "Blocked on T.9.",
    ]


def test_a_session_ignite_cannot_name_is_let_go() -> None:
    assert turn_ended.command({"stop_hook_active": True}, {}) is None


def test_the_last_message_falls_back_to_the_transcript(tmp_path: Path) -> None:
    transcript = tmp_path / "t.jsonl"
    lines = [
        {"message": {"role": "assistant", "content": "first"}},
        {"message": {"role": "user", "content": "hi"}},
        {
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Landing refused by T.9."},
                    {"type": "tool_use", "name": "Bash"},
                ],
            }
        },
        {"message": {"role": "assistant", "content": [{"type": "tool_use"}]}},
    ]
    transcript.write_text(
        "\n".join(json.dumps(line) for line in lines) + "\nnot json\n"
    )
    event = {"transcript_path": str(transcript)}
    assert turn_ended.last_message(event) == "Landing refused by T.9."
    assert turn_ended.last_message({"transcript_path": str(tmp_path / "x")}) == ""


def test_only_a_block_verdict_becomes_a_decision() -> None:
    block = "block    You are ending your turn while you hold T.1.\n"
    assert turn_ended.decision(block) == {
        "decision": "block",
        "reason": "You are ending your turn while you hold T.1.",
    }
    assert turn_ended.decision("pass     holds no ticket\n") is None
    assert turn_ended.decision("released T.1 from local_w1\n") is None
    assert turn_ended.decision("") is None
