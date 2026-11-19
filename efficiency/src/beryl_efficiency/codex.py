"""The Codex source: session logs (docs/efficiency-dashboard.md, "Sources").

A log is `sessions/<yyyy>/<mm>/<dd>/rollout-<time>-<id>.jsonl` (or the same file name in
`archived_sessions/`), one JSON object per line with `timestamp`, `type` and `payload`. The
`<id>` is the thread id, which Enact records on a ticket as `enact_session`, so it is the
transcript's id. Lines that matter:

- `session_meta` (the first line) has the working directory, `cwd`. Only logs whose `cwd` is
  the main checkout or below it are read.
- `event_msg` lines whose payload is a `token_count` with `info` carry `last_token_usage`, the
  tokens of the request just made, and `total_token_usage`, the running total. `input_tokens`
  includes the cached part, and `output_tokens` includes reasoning. A request is keyed by the
  running total's `total_tokens`, since Codex sometimes repeats a `token_count` line.
- `turn_context` lines name the turn's `model`. `event_msg` payloads `task_complete` and
  `turn_aborted` end a turn, and an `item_completed` for a `UserMessage` is a prompt.
- `response_item` lines of type `function_call` and `custom_tool_call` are tool calls, ended
  by the `..._output` line with the same `call_id`. Codex runs shell commands through a
  `custom_tool_call` named `exec` whose input is JavaScript calling
  `tools.exec_command({cmd: "..."})`; the commands are read from the string literals after
  `cmd:`.

Only commands, sizes and times are kept: no message text and no tool output. A request's model
and whether it ended a turn are on other lines than its tokens, so they are copied onto the
requests in `finish`, from the `codex_marks` table.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

from . import claude, kinds, paths, timing, tokens, windows
from .ingest import LineSource, Row

PROVIDER = "codex"
# The thread id at the end of a log's file name.
THREAD: re.Pattern[str] = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$")
# Where a JavaScript call gives a shell command: `cmd: "..."`, or the same key in JSON.
CMD: re.Pattern[str] = re.compile(r"""["']?\bcmd["']?\s*:\s*(?=["'`])""")
# Tools that run one shell command, given as JSON arguments with `cmd` or `command`.
SHELL_FUNCTIONS: frozenset[str] = frozenset({"exec_command", "shell", "local_shell"})
# Lines without one of these are not parsed.
WANTED: tuple[bytes, ...] = (
    b"token_count",
    b"_call",
    b"turn_context",
    b"UserMessage",
    b"task_complete",
    b"turn_aborted",
)
ESCAPES: dict[str, str] = {"n": "\n", "t": "\t", "r": "\r", "0": "\0"}


def thread_of(path: Path) -> str:
    """The thread id in a log's file name."""
    found = THREAD.search(path.stem)
    if found is None:
        raise ValueError(f"{path.name} does not end in a thread id")
    return found.group(0)


def literal(text: str, start: int) -> str | None:
    """The JavaScript string literal that starts at `text[start]` (a quote), decoded, or None
    when it is not closed."""
    quote = text[start]
    out: list[str] = []
    index = start + 1
    while index < len(text):
        char = text[index]
        if char == quote:
            return "".join(out)
        if char == "\\" and index + 1 < len(text):
            index += 1
            char = ESCAPES.get(text[index], text[index])
        out.append(char)
        index += 1
    return None


def shell_commands(code: str) -> list[str]:
    """The shell commands a script gives as `cmd: "<literal>"`, in order. A command built
    from variables or concatenation is not found."""
    found: list[str] = []
    for match in CMD.finditer(code):
        value = literal(code, match.end())
        if value:
            found.append(value)
    return found


def output_text(output: object) -> str:
    """The text of a tool's output, which is a string or a list of `input_text` items."""
    if isinstance(output, str):
        return output
    if isinstance(output, list):
        return "\n".join(
            str(item.get("text") or "") for item in output if isinstance(item, dict)
        )
    return ""


def counted(usage: dict[str, object]) -> dict[str, int]:
    """A request's tokens as the `requests` table has them: Codex's input includes the cached
    part, and its output includes reasoning."""
    total = claude.number(usage, "input_tokens")
    cached = claude.number(usage, "cached_input_tokens")
    return {
        "input_tokens": max(total - cached, 0),
        "cache_write_tokens": claude.number(usage, "cache_write_input_tokens"),
        "cache_read_tokens": cached,
        "output_tokens": claude.number(usage, "output_tokens"),
        "context_tokens": total,
    }


class Rollouts(LineSource):
    """Every Codex log whose working directory is the main checkout or below it."""

    name = "codex"

    def __init__(self, roots: list[Path], checkout: Path, host: str) -> None:
        self.roots = roots
        self.checkout = str(checkout)
        self.host = host
        # Whether each log is ours. A log's first line never changes, so this is asked once.
        self.ours: dict[Path, bool] = {}

    def belongs(self, path: Path) -> bool:
        if path not in self.ours:
            self.ours[path] = self.cwd_matches(path)
        return self.ours[path]

    def cwd_matches(self, path: Path) -> bool:
        try:
            with open(path, "rb") as handle:
                first = json.loads(handle.readline())
            cwd = str(first["payload"].get("cwd") or "")
            thread_of(path)
        except (OSError, ValueError, KeyError, TypeError, AttributeError):
            return False
        return paths.inside(cwd, self.checkout)

    def files(self) -> list[Path]:
        found: list[Path] = []
        for root in self.roots:
            try:
                found += root.glob("**/rollout-*.jsonl")
            except OSError:
                continue
        return sorted(p for p in found if self.belongs(p))

    def parse(self, path: Path, line: bytes) -> list[Row]:
        if not any(w in line for w in WANTED):
            return []
        record = json.loads(line)
        if not isinstance(record, dict):
            raise TypeError("not an object")
        payload = record.get("payload")
        if not isinstance(payload, dict):
            return []
        key: dict[str, object] = {
            "host": self.host,
            "provider": PROVIDER,
            "transcript": thread_of(path),
        }
        time = claude.text_field(record, "timestamp")
        kind = record.get("type")
        found = payload.get("type")
        if kind == "turn_context":
            model = payload.get("model")
            if not isinstance(model, str):
                return []
            mark = {**key, "time": time, "kind": "model", "value": model}
            return [("codex_marks", mark)]
        if kind == "event_msg":
            return self.event(key, time, payload, found)
        if kind == "response_item" and found in ("function_call", "custom_tool_call"):
            return self.call(key, time, payload, str(found))
        if kind == "response_item" and found in (
            "function_call_output",
            "custom_tool_call_output",
        ):
            return self.result(key, time, payload)
        return []

    def event(
        self,
        key: dict[str, object],
        time: str,
        payload: dict[str, object],
        found: object,
    ) -> list[Row]:
        if found == "token_count":
            info = payload.get("info")
            if not isinstance(info, dict):
                return []
            last, total = info.get("last_token_usage"), info.get("total_token_usage")
            if not isinstance(last, dict) or not isinstance(total, dict):
                raise TypeError("token_count without usage")
            message = str(claude.number(total, "total_tokens"))
            return [
                (
                    "requests",
                    {**key, "message": message, "time": time, **counted(last)},
                )
            ]
        if found in ("task_complete", "turn_aborted"):
            mark = {**key, "time": time, "kind": "turn_end", "value": ""}
            rows: list[Row] = [("codex_marks", mark)]
            if found == "turn_aborted":
                prompt = {**key, "time": time, "kind": "person", "detail": "interrupt"}
                rows.append(("prompts", prompt))
            return rows
        item = payload.get("item")
        if (
            found == "item_completed"
            and isinstance(item, dict)
            and item.get("type") == "UserMessage"
        ):
            prompt = {**key, "time": time, "kind": "person", "detail": None}
            return [("prompts", prompt)]
        return []

    def call(
        self, key: dict[str, object], time: str, payload: dict[str, object], kind: str
    ) -> list[Row]:
        call = claude.text_field(payload, "call_id")
        name = str(payload.get("name") or "")
        commands: list[str] = []
        if kind == "custom_tool_call":
            code = payload.get("input")
            commands = shell_commands(code) if isinstance(code, str) else []
        elif name in SHELL_FUNCTIONS:
            arguments = payload.get("arguments")
            given = json.loads(arguments) if isinstance(arguments, str) else None
            value = (
                (given.get("cmd") or given.get("command"))
                if isinstance(given, dict)
                else None
            )
            if isinstance(value, list):
                value = " ".join(str(v) for v in value)
            commands = [value] if isinstance(value, str) and value else []
        command = kinds.without_heredocs("\n".join(commands))
        rows: list[Row] = [
            (
                "tool_calls",
                {
                    **key,
                    "call": call,
                    "start": time,
                    "tool": "Bash" if commands else name,
                    "command": command[: claude.COMMAND_CHARACTERS] or None,
                },
            )
        ]
        rows += [
            (
                "enact_events",
                {**key, "call": call, "verb": verb, "bead": bead, "time": time},
            )
            for verb, bead in claude.enact_commands(command)
        ]
        return rows

    def result(
        self, key: dict[str, object], time: str, payload: dict[str, object]
    ) -> list[Row]:
        if "call_id" not in payload:
            return []  # A message between threads, written in the shape of an output.
        call = claude.text_field(payload, "call_id")
        output = payload.get("output")
        text = output_text(output)
        rows: list[Row] = [
            (
                "tool_results",
                {
                    **key,
                    "call": call,
                    "end": time,
                    "result_bytes": claude.size(output),
                    "compile_seconds": claude.compile_seconds({"stdout": text}),
                    "deferred": claude.deferred({"stdout": text}),
                },
            )
        ]
        # A script's output holds the command's output as an escaped JSON string.
        found = claude.NEXT.search(text) or claude.NEXT.search(text.replace('\\"', '"'))
        if found:
            event = {
                **key,
                "call": call,
                "verb": "next",
                "bead": found[1],
                "time": time,
            }
            rows.append(("enact_events", event))
        return rows

    def finish(self, db: windows.Connection, changed: list[Path]) -> int:
        found: list[tuple[str, str | None]] = [(thread_of(p), None) for p in changed]
        for transcript, _ in found:
            copy_marks(db, self.host, transcript)
        written = windows.derive(db, self.host, PROVIDER, found)
        tops = sorted(t for t, _ in found)
        written += timing.refresh(db, self.host, PROVIDER, tops)
        return written + tokens.refresh(db, self.host, PROVIDER, tops)


def copy_marks(db: windows.Connection, host: str, transcript: str) -> None:
    """Sets each request's `model` to the one its turn named last before it (the first
    named, for a request before any), and its `stop_reason` to `end_turn` when a turn ended
    between it and the next request, else `tool_use`."""
    key = (host, PROVIDER, transcript)
    marks = db.execute(
        "SELECT time, kind, value FROM codex_marks WHERE host = ? AND provider = ? "
        "AND transcript = ? ORDER BY time",
        key,
    ).fetchall()
    models = [(str(m[0]), str(m[2])) for m in marks if m[1] == "model"]
    ends = [str(m[0]) for m in marks if m[1] == "turn_end"]
    requests = [
        str(r[0])
        for r in db.execute(
            "SELECT time FROM requests WHERE host = ? AND provider = ? "
            "AND transcript = ? ORDER BY time",
            key,
        )
    ]
    for index, time in enumerate(requests):
        later = requests[index + 1] if index + 1 < len(requests) else None
        before = [m for t, m in models if t <= time]
        model = before[-1] if before else (models[0][1] if models else None)
        stops = any(time <= e and (later is None or e < later) for e in ends)
        db.execute(
            "UPDATE requests SET model = ?, stop_reason = ? WHERE host = ? "
            "AND provider = ? AND transcript = ? AND time = ?",
            (model, "end_turn" if stops else "tool_use", *key, time),
        )
