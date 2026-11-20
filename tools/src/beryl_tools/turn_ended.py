"""A Claude Code `Stop` hook that keeps a pool worker from ending its turn holding its ticket.

A pool worker could end its turn with its ticket still leased, and nothing noticed until
Ignite's stall rule checked on it about 45 minutes later. This hook runs at the end of every
turn (`.claude/settings.json`, `Stop`) and relays the turn's end to `ignite turn-ended`, which
decides:

- `pass`: the session may stop. Only a worker of the machine's pool that holds a ticket, has no
  `ignite` command running and is no justiciar is ever stopped.
- `block`: the first stop with the ticket held. The hook prints Claude Code's block decision
  with Ignite's text, which names the ticket and tells the worker to land or release it.
- `released`: the hook had already blocked once this turn (`stop_hook_active`), or the worker's
  title starts with ⛔ (stopped unfinished), so Ignite released the ticket itself, using the
  worker's last message as the note, and for a ⛔ title also sent for a justiciar.

The session is the one Ignite records for the ticket lease: `IGNITE_SESSION`, else the desktop
app's `CLAUDE_CODE_HOST_SESSION_ID` (`local_...`), the same variables the `ignite` client reads.
The hook input's `session_id` is Claude Code's own ID, which Ignite never sees. A session with
neither variable, a missing `ignite` or a failed call lets the turn end.

It uses only the standard library and imports nothing from `beryl_tools`, so it runs as a
plain script with the system `python3`, without starting `uv`.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from collections.abc import Mapping

SESSION_VARIABLES: tuple[str, ...] = ("IGNITE_SESSION", "CLAUDE_CODE_HOST_SESSION_ID")

# Seconds to wait for Ignite, below the hook's own timeout in `.claude/settings.json`. A
# release saves the tree and asks the hub, which can take a few seconds.
IGNITE_TIMEOUT_S: float = 50.0

# Characters of the worker's last message sent as the release note.
NOTE_CHARS: int = 4000


def session(env: Mapping[str, str]) -> str | None:
    """The session Ignite knows this one by, or None outside the desktop app and Ignite."""
    for name in SESSION_VARIABLES:
        value = env.get(name, "").strip()
        if value:
            return value
    return None


def text_of(content: object) -> str:
    """The text of a transcript message's `content`: a string, or a list of blocks."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            text
            for block in content
            if isinstance(block, dict)
            and block.get("type") == "text"
            and isinstance(text := block.get("text"), str)
        )
    return ""


def last_message(event: Mapping[str, object]) -> str:
    """The worker's last message: the hook input's `last_assistant_message` when Claude Code
    sends it, else the last assistant text in the transcript."""
    given = event.get("last_assistant_message")
    if isinstance(given, str) and given.strip():
        return given.strip()
    path = event.get("transcript_path")
    if not isinstance(path, str):
        return ""
    found = ""
    try:
        with open(path, encoding="utf-8") as transcript:
            for line in transcript:
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                message = entry.get("message") if isinstance(entry, dict) else None
                if isinstance(message, dict) and message.get("role") == "assistant":
                    text = text_of(message.get("content")).strip()
                    if text:
                        found = text
    except OSError:
        return ""
    return found


def command(event: Mapping[str, object], env: Mapping[str, str]) -> list[str] | None:
    """The `ignite turn-ended` call for this stop, or None when there is no session to name."""
    name = session(env)
    if name is None:
        return None
    args = ["ignite", "turn-ended", "--session", name]
    if event.get("stop_hook_active") is True:
        args.append("--again")
    # Sent at the first stop too: a worker titled ⛔ is released at its first stop, and its
    # last message is the release note and the justiciar report's reason.
    note = last_message(event)[:NOTE_CHARS]
    if note:
        args += ["--note", note]
    return args


def decision(output: str) -> dict[str, str] | None:
    """Claude Code's block decision when Ignite's verdict is `block`, else None."""
    verdict, _, reason = output.strip().partition(" ")
    if verdict != "block" or not reason.strip():
        return None
    return {"decision": "block", "reason": reason.strip()}


def main() -> int:
    """Read the hook event from stdin, ask Ignite, and print a block decision if it says so."""
    try:
        event = json.load(sys.stdin)
    except json.JSONDecodeError:
        return 0
    if not isinstance(event, dict):
        return 0
    args = command(event, os.environ)
    if args is None:
        return 0
    try:
        done = subprocess.run(
            args,
            capture_output=True,
            text=True,
            timeout=IGNITE_TIMEOUT_S,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return 0
    if done.returncode != 0:
        return 0
    block = decision(done.stdout)
    if block is not None:
        print(json.dumps(block, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
