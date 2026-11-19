"""A Claude Code hook that reminds a session to set its title right after `ignite start`.

`ignite start` prints the session title to set, but in the Claude desktop app the
`set_session_title` tool is deferred: a session has to load it with ToolSearch first and call
it in a later request. A session that loaded it alongside its first file reads went on to the
code and never called it, so its title kept the app's generated name. This hook runs after
every Bash call (`.claude/settings.json`, `PostToolUse`). When the command ran `ignite start`
and its output has a `title` line, it adds a note to the session's context naming the exact
title to set before anything else. Any other call passes through with no output.

It uses only the standard library and imports nothing from `beryl_tools`, so it runs as a
plain script with the system `python3`, without starting `uv`.
"""

from __future__ import annotations

import json
import re
import sys

TITLE_LINE: re.Pattern[str] = re.compile(r"^title {2,}(\S.*)$", re.MULTILINE)


def output_text(response: object) -> str:
    """The text a Bash tool call printed, from the hook input's `tool_response`."""
    if isinstance(response, str):
        return response
    if isinstance(response, dict):
        return "\n".join(
            value
            for key in ("stdout", "stderr", "output")
            if isinstance(value := response.get(key), str)
        )
    return ""


def reminder(event: dict[str, object]) -> str | None:
    """The note for a Bash call that ran `ignite start`, or None for any other call."""
    tool_input = event.get("tool_input")
    command = tool_input.get("command") if isinstance(tool_input, dict) else None
    if not isinstance(command, str) or "ignite start" not in command:
        return None
    found = TITLE_LINE.search(output_text(event.get("tool_response")))
    if found is None:
        return None
    title = found.group(1).strip()
    return (
        f"Set this session's title to `{title}` now, before any other tool call. "
        "In the Claude desktop app, load `mcp__ccd_session_mgmt__set_session_title` with "
        "ToolSearch in a message of its own, then call it with session_id `self`. "
        "In Codex, call `set_thread_title`."
    )


def main() -> int:
    """Read the hook event from stdin and print the note as hook JSON when one applies."""
    try:
        event = json.load(sys.stdin)
    except json.JSONDecodeError:
        return 0
    note = reminder(event) if isinstance(event, dict) else None
    if note is not None:
        output = {
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "additionalContext": note,
            }
        }
        print(json.dumps(output, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
