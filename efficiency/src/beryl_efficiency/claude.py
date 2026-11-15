"""The Claude Code sources: transcripts and the desktop app's session records
(docs/efficiency-dashboard.md, "Sources").

A transcript is `~/.claude/projects/<project>/<id>.jsonl`, where `<project>` is the working
directory with every character that is not a letter or digit replaced by `-`; a subagent's is
`<id>/subagents/agent-<n>.jsonl` beside it. Lines that matter:

- `"type": "assistant"` lines carry `timestamp` and `message` with `id`, `model`, `usage` and
  `content` blocks, of which `tool_use` blocks are tool calls. A message with several content
  blocks is written once per block, each line with the same `id` and `usage`, so a request is
  keyed by its message id and counted once.
- `"type": "user"` lines whose `content` holds `tool_result` blocks end those tool calls at
  their `timestamp`.

Only commands, file paths and sizes are kept: no message text and no tool output.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable
from datetime import datetime, timezone
from pathlib import Path

from . import windows
from .ingest import FileSource, LineSource, Row

PROVIDER = "claude"
# How many characters of a command the store keeps.
COMMAND_CHARACTERS = 300
# A bead id as Enact prints it, such as `by-ig5e.2`.
BEAD = r"[a-z][a-z0-9]*-[a-z0-9]+(?:\.[0-9]+)*"
# An Enact command at the start of a shell command or after `&&`, `;`, `|` or `(`, maybe after
# `eval`, `timeout <n>` or `NAME=value`, run through the `enact` alias, `python enact.py` or a
# shell variable holding either (sessions often write `E='python3 enact.py'; eval $E start`).
# A command that only mentions one, such as `grep 'enact start'`, does not match.
ENACT: re.Pattern[str] = re.compile(
    r"(?:^|[;&|(\n])\s*(?:(?:eval|timeout\s+\S+|[A-Za-z_]+=\S*)\s+)*"
    r"(?:\"?\S*python[0-9.]*\"?\s+)?"
    r"(?:\"?(?:\S*/)?enact(?:\.py)?\"?|\"?\$\{?[A-Za-z_]+\}?\"?)\s+"
    r"(?:(?P<verb>start|claim|close)\s+(?P<bead>" + BEAD + r")"
    r"|worktree\s+create\s+(?P<name>[\w.-]+))"
)
# The bead `enact close --continue` claimed, in its printed result: the first `"id"` key in
# its `"next"` object, which may follow other keys. A quote inside a JSON string is escaped,
# so text in a description cannot match.
NEXT: re.Pattern[str] = re.compile(
    r'"next":\s*\{.*?"id":\s*"(' + BEAD + r')"', re.DOTALL
)
# Lines without one of these are neither requests nor tool results, and are not parsed.
WANTED: tuple[bytes, ...] = (b'"usage"', b'"tool_result"')


def project_name(path: Path) -> str:
    """The transcript folder Claude Code uses for the working directory `path`."""
    return re.sub(r"[^A-Za-z0-9]", "-", str(path))


def transcript_of(path: Path) -> tuple[str, str | None]:
    """A transcript file's id and, for a subagent, the id of the transcript that started it.
    A subagent's id is `<parent>/<file stem>`, since its file name alone is not unique.
    """
    if path.parent.name == "subagents":
        parent = path.parent.parent.name
        return f"{parent}/{path.stem}", parent
    return path.stem, None


def enact_commands(command: str) -> list[tuple[str, str]]:
    """The `(verb, bead)` of each Enact command in a shell command, with `open` for start and
    claim, `worktree` with the worktree's name for `enact worktree create`, and `continue`
    beside the `close` of an `enact close --continue`."""
    found: list[tuple[str, str]] = []
    for match in ENACT.finditer(command):
        name = match.group("name")
        if name:
            found.append(("worktree", name))
            continue
        verb, bead = match.group("verb"), match.group("bead")
        found.append(("close" if verb == "close" else "open", bead))
        rest = re.split(r"[;&|\n]", command[match.end() :], maxsplit=1)[0]
        if verb == "close" and "--continue" in rest.split():
            found.append(("continue", bead))
    return found


def size(value: object) -> int:
    """The size in bytes of a tool's output: the shell's stdout and stderr, else the result as
    the model read it."""
    if isinstance(value, str):
        return len(value.encode())
    if isinstance(value, dict) and ("stdout" in value or "stderr" in value):
        return sum(size(value.get(k) or "") for k in ("stdout", "stderr"))
    if isinstance(value, list):
        return sum(
            size(b.get("text") or "") if isinstance(b, dict) else size(b) for b in value
        )
    return len(json.dumps(value).encode())


def number(usage: dict[str, object], key: str) -> int:
    value = usage.get(key) or 0
    if not isinstance(value, int):
        raise TypeError(f"{key} is not a number")
    return value


def text_field(record: dict[str, object], key: str) -> str:
    value = record[key]
    if not isinstance(value, str):
        raise TypeError(f"{key} is not text")
    return value


def target(tool: str, given: object) -> str | None:
    """What a tool call acted on, as the store keeps it: a shell command's first characters,
    or the path or pattern another tool was given. Never message text or file contents.
    """
    if not isinstance(given, dict):
        return None
    for key in ("command", "file_path", "notebook_path", "path", "pattern", "url"):
        value = given.get(key)
        if isinstance(value, str) and (key != "command" or tool == "Bash"):
            return value[:COMMAND_CHARACTERS]
    return None


class Transcripts(LineSource):
    """Every Claude Code transcript whose working directory is the main checkout or below it,
    such as a worktree."""

    name = "claude"

    def __init__(self, root: Path, checkout: Path, host: str) -> None:
        self.root = root
        self.project: str = project_name(checkout)
        self.host = host

    def folders(self) -> list[Path]:
        try:
            return sorted(
                p
                for p in self.root.iterdir()
                if p.name == self.project or p.name.startswith(self.project + "-")
            )
        except OSError:
            return []

    def files(self) -> list[Path]:
        found: list[Path] = []
        for folder in self.folders():
            found += sorted(folder.glob("*.jsonl"))
            found += sorted(folder.glob("*/subagents/*.jsonl"))
        return found

    def parse(self, path: Path, line: bytes) -> list[Row]:
        if not any(w in line for w in WANTED):
            return []
        record = json.loads(line)
        if not isinstance(record, dict):
            raise TypeError("not an object")
        kind = record.get("type")
        if kind not in ("assistant", "user"):
            return []
        message = record["message"]
        if not isinstance(message, dict):
            raise TypeError("message is not an object")
        content = message.get("content")
        blocks = (
            [b for b in content if isinstance(b, dict)]
            if isinstance(content, list)
            else []
        )
        key: dict[str, object] = {
            "host": self.host,
            "provider": PROVIDER,
            "transcript": transcript_of(path)[0],
        }
        time = text_field(record, "timestamp")
        if kind == "assistant":
            return self.assistant(key, time, message, blocks)
        output = record.get("toolUseResult")
        rows: list[Row] = []
        for block in blocks:
            if block.get("type") != "tool_result":
                continue
            call = text_field(block, "tool_use_id")
            rows.append(
                (
                    "tool_results",
                    {
                        **key,
                        "call": call,
                        "end": time,
                        "result_bytes": size(
                            output if output is not None else block.get("content")
                        ),
                    },
                )
            )
            stdout = output.get("stdout") if isinstance(output, dict) else None
            found = NEXT.search(stdout) if isinstance(stdout, str) else None
            if found:
                rows.append(
                    (
                        "enact_events",
                        {
                            **key,
                            "call": call,
                            "verb": "next",
                            "bead": found[1],
                            "time": time,
                        },
                    )
                )
        return rows

    def assistant(
        self,
        key: dict[str, object],
        time: str,
        message: dict[str, object],
        blocks: list[dict[str, object]],
    ) -> list[Row]:
        usage = message["usage"]
        if not isinstance(usage, dict):
            raise TypeError("usage is not an object")
        tokens = {
            "input_tokens": number(usage, "input_tokens"),
            "cache_write_tokens": number(usage, "cache_creation_input_tokens"),
            "cache_read_tokens": number(usage, "cache_read_input_tokens"),
            "output_tokens": number(usage, "output_tokens"),
        }
        model = message.get("model")
        rows: list[Row] = [
            (
                "requests",
                {
                    **key,
                    "message": text_field(message, "id"),
                    "time": time,
                    "model": model if isinstance(model, str) else None,
                    **tokens,
                    "context_tokens": tokens["input_tokens"]
                    + tokens["cache_write_tokens"]
                    + tokens["cache_read_tokens"],
                },
            )
        ]
        for block in blocks:
            if block.get("type") != "tool_use":
                continue
            call = text_field(block, "id")
            tool = str(block.get("name") or "")
            given = block.get("input")
            rows.append(
                (
                    "tool_calls",
                    {
                        **key,
                        "call": call,
                        "start": time,
                        "tool": tool,
                        "command": target(tool, given),
                    },
                )
            )
            command = given.get("command") if isinstance(given, dict) else None
            if tool == "Bash" and isinstance(command, str):
                rows += [
                    (
                        "enact_events",
                        {**key, "call": call, "verb": verb, "bead": bead, "time": time},
                    )
                    for verb, bead in enact_commands(command)
                ]
        return rows

    def finish(self, db: windows.Connection, changed: list[Path]) -> int:
        return windows.derive(
            db, self.host, PROVIDER, [transcript_of(p) for p in changed]
        )


def iso_from_milliseconds(value: object) -> str | None:
    if not isinstance(value, (int, float)):
        return None
    moment = datetime.fromtimestamp(value / 1000, timezone.utc)
    return moment.isoformat(timespec="milliseconds").replace("+00:00", "Z")


class Sessions(FileSource):
    """The desktop app's session records whose working directory is the main checkout or
    below it. Each maps a session id (`local_<uuid>`, what Enact records on a bead as
    `enact_session`) to its current transcript (`cliSessionId`) and its earlier ones."""

    name = "claude-sessions"

    def __init__(
        self,
        root: Path,
        checkout: Path,
        host: str,
        files: Callable[[], list[Path]] | None = None,
    ) -> None:
        self.root = root
        self.checkout = str(checkout)
        self.host = host
        self._files = files

    def files(self) -> list[Path]:
        if self._files is not None:
            return self._files()
        try:
            return sorted(self.root.glob("*/*/local_*.json"))
        except OSError:
            return []

    def parse(self, path: Path, data: bytes) -> list[Row]:
        record = json.loads(data)
        if not isinstance(record, dict):
            raise TypeError("not an object")
        cwd = str(record.get("originCwd") or record.get("cwd") or "")
        if cwd != self.checkout and not cwd.startswith(self.checkout + "/"):
            return []
        session = text_field(record, "sessionId")
        title = record.get("title")
        model = record.get("model")
        spawned = record.get("spawnedFrom")
        parent = spawned.get("sessionId") if isinstance(spawned, dict) else None
        common: dict[str, object] = {
            "host": self.host,
            "provider": PROVIDER,
            "session": session,
            "title": title if isinstance(title, str) else None,
            "model": model if isinstance(model, str) else None,
            "spawned_from": parent if isinstance(parent, str) else None,
        }
        rows: list[Row] = []
        current = record.get("cliSessionId")
        if isinstance(current, str) and current:
            rows.append(
                ("sessions", {**common, "transcript": current, "left_at": None})
            )
        prior = record.get("priorCliSessionIds")
        left = record.get("priorCliSessionLeftAt")
        left_at = left if isinstance(left, dict) else {}
        for transcript in prior if isinstance(prior, list) else []:
            if isinstance(transcript, str) and transcript:
                rows.append(
                    (
                        "sessions",
                        {
                            **common,
                            "transcript": transcript,
                            "left_at": iso_from_milliseconds(left_at.get(transcript)),
                        },
                    )
                )
        return rows
