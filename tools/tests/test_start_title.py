"""The post-`ignite start` hook names the printed title, and stays silent otherwise."""

import io
import json

import pytest

from beryl_tools import start_title

START_OUTPUT = """\
ticket   TQ44S.9.48  Counter names, untap lock
tree     /Users/dthurn/beryl/.worktrees/tree-07  at staging 8c1f2ab
title    ⚒️ [TQ44S.9.48] Counter names, untap lock
next     set the title first, then cd to the tree, read the files and edit
"""


def bash_event(command: str, stdout: str) -> dict[str, object]:
    return {
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": False},
    }


def test_ignite_start_names_the_title() -> None:
    note = start_title.reminder(
        bash_event('ignite start "x" 2>&1 | tail -20', START_OUTPUT)
    )
    assert note is not None
    assert "`⚒️ [TQ44S.9.48] Counter names, untap lock`" in note
    assert "set_session_title" in note
    assert "same message as your first reads" in note


def test_other_commands_and_refusals_are_silent() -> None:
    assert start_title.reminder(bash_event("git status", START_OUTPUT)) is None
    refused = (
        "refused  TQ44S.9.31 is reserved for a worker the dispatcher is starting\n"
    )
    assert start_title.reminder(bash_event("ignite start TQ44S.9.31", refused)) is None


def test_main_prints_hook_json(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    event = bash_event("ignite start TQ44S.9.48", START_OUTPUT)
    monkeypatch.setattr("sys.stdin", io.StringIO(json.dumps(event)))
    assert start_title.main() == 0
    output = json.loads(capsys.readouterr().out)
    assert output["hookSpecificOutput"]["hookEventName"] == "PostToolUse"
    assert "TQ44S.9.48" in output["hookSpecificOutput"]["additionalContext"]
