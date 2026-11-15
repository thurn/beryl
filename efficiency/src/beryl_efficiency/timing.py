"""Time accounting: each window's wall-clock seconds split so that every second falls in exactly
one category (docs/efficiency-dashboard.md, "Time accounting").

At any instant a window is in one of six categories; where more than one applies, the first
in this order wins:

- `person`: a question to the user is unanswered (the `AskUserQuestion` tool), or the session
  ended its turn and the next prompt is one a person typed.
- `queued`: a tool call is waiting for a CPU lease (its `queued_seconds`, filled by the CPU
  accounting; zero until then).
- `tool`: a tool call is running, by kind of command. Seconds when several run at once are
  divided equally among them. A cargo command's seconds are split into `compile` and `run`
  parts by the share its `Finished` lines name.
- `model`: the model is generating. This is every second no other category claims.
- `background`: the session ended its turn and the next prompt reports that a background
  command or another session finished, by the kind of that command (`session` for another
  session).
- `idle`: the session ended its turn and the next prompt is anything else, such as a
  scheduled wake-up.

A turn ends at a request that did not stop for a tool, or at an interrupt. A subagent adds no
time: its work is inside the parent's `Agent` call, so only a top-level transcript's rows are
read.
"""

from __future__ import annotations

import sqlite3
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime

from . import kinds

# The categories in order of precedence. Agent minutes are `queued`, `tool`, `model` and
# `background`; `person` and `idle` are reported beside them.
CATEGORIES: tuple[str, ...] = (
    "person",
    "queued",
    "tool",
    "model",
    "background",
    "idle",
)
AGENT_CATEGORIES: frozenset[str] = frozenset({"queued", "tool", "model", "background"})
# The tool that puts a question to the user.
QUESTION_TOOLS: frozenset[str] = frozenset({"AskUserQuestion"})
# Raised whenever a change to this file changes the split, so that the next ingest gives
# every call its kind and splits every window again, as it does when the kinds table changes.
VERSION = 1
# Stop reasons of a request that ends a turn: anything but stopping to run a tool.
TOOL_STOP = "tool_use"

Key = tuple[str, str, str]  # category, kind, part


@dataclass
class Span:
    """A stretch of a transcript in one category, from `start` to `end` in seconds since the
    epoch. A tool call's span carries its call id, its compile and queued seconds."""

    start: float
    end: float
    category: str
    kind: str
    call: str | None = None
    compile_seconds: float = 0.0
    queued_seconds: float = 0.0


def seconds(value: str) -> float:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()


def spans_of(
    db: sqlite3.Connection,
    host: str,
    provider: str,
    transcript: str,
    heavy: frozenset[str] = frozenset(),
) -> list[Span]:
    """The tool calls and turn-end waits of a top-level transcript. Only a call of a `heavy`
    kind keeps its compile seconds: a light one, such as `tail` on a build log, printed
    cargo's `Finished` line without building."""
    key = (host, provider, transcript)
    found: list[Span] = []
    calls = db.execute(
        """SELECT call, start, end, tool, coalesce(kind, ?), coalesce(compile_seconds, 0),
            queued_seconds FROM tool_calls
        WHERE host = ? AND provider = ? AND transcript = ? AND end IS NOT NULL""",
        (kinds.OTHER, *key),
    ).fetchall()
    kind_of: dict[str, str] = {}
    for call, start, end, tool, kind, compiled, queued in calls:
        kind_of[str(call)] = str(kind)
        low, high = seconds(str(start)), seconds(str(end))
        if high <= low:
            continue
        question = str(tool) in QUESTION_TOOLS
        found.append(
            Span(
                low,
                high,
                "person" if question else "tool",
                "question" if question else str(kind),
                str(call),
                float(compiled) if str(kind) in heavy else 0.0,
                float(queued),
            )
        )
    # Every record before a prompt, as (time, ends a turn).
    records: list[tuple[float, bool]] = [
        (seconds(str(r[0])), r[1] is not None and str(r[1]) != TOOL_STOP)
        for r in db.execute(
            "SELECT time, stop_reason FROM requests WHERE host = ? AND provider = ? "
            "AND transcript = ?",
            key,
        )
    ]
    records += [
        (seconds(str(r[0])), False)
        for r in db.execute(
            """SELECT start FROM tool_calls WHERE host = ?1 AND provider = ?2
                AND transcript = ?3
            UNION ALL SELECT end FROM tool_calls WHERE host = ?1 AND provider = ?2
                AND transcript = ?3 AND end IS NOT NULL""",
            key,
        )
    ]
    prompts = [
        (seconds(str(r[0])), str(r[1]), r[2])
        for r in db.execute(
            "SELECT time, kind, detail FROM prompts WHERE host = ? AND provider = ? "
            "AND transcript = ? ORDER BY time",
            key,
        )
    ]
    # An interrupt also ends a turn.
    records += [(t, detail == "interrupt") for t, _, detail in prompts]
    records.sort()
    tools = [(s.start, s.end) for s in found if s.call is not None]
    index = 0
    last: tuple[float, bool] | None = None
    for time, kind, detail in prompts:
        while index < len(records) and records[index][0] < time:
            last = records[index]
            index += 1
        if last is None or not last[1] or last[0] >= time:
            continue
        if any(low < time < high for low, high in tools):
            continue  # A tool was still running: the turn had not ended.
        named = kind_of.get(str(detail), str(detail)) if kind == "background" else ""
        found.append(Span(last[0], time, kind, named))
    return found


def split(spans: list[Span], start: float, end: float) -> dict[Key, float]:
    """The seconds of `start` to `end` by category, kind and part: a sweep over the points
    where any span begins or ends, giving each stretch to the category that wins there and
    dividing it equally among that category's spans."""
    inside = [s for s in spans if s.end > start and s.start < end]
    edges: list[tuple[float, int, int]] = []
    for number, span in enumerate(inside):
        edges.append((max(span.start, start), 1, number))
        edges.append((min(span.end, end), -1, number))
    edges.sort()
    totals: dict[Key, float] = defaultdict(float)
    per_call: dict[int, float] = defaultdict(float)
    active: dict[str, set[int]] = {c: set() for c in CATEGORIES}
    reached = start
    for point, step, number in [*edges, (end, 0, -1)]:
        if point > reached:
            width = point - reached
            # The model owns every second no span claims. Its place in the order matters
            # only in principle: turn-end spans never overlap a request being generated.
            winner = next(
                (c for c in CATEGORIES if c != "model" and active[c]), "model"
            )
            if winner == "model":
                totals[("model", "", "")] += width
            else:
                share = width / len(active[winner])
                for running in active[winner]:
                    if winner == "tool":
                        per_call[running] += share
                    else:
                        totals[(winner, inside[running].kind, "")] += share
            reached = point
        if step == 1:
            active[inside[number].category].add(number)
        elif step == -1:
            active[inside[number].category].discard(number)
    for number, span in enumerate(inside):
        share = per_call.get(number)
        if not share:
            continue
        length = span.end - span.start
        queued = share * min(span.queued_seconds / length, 1.0)
        if queued:
            totals[("queued", span.kind, "")] += queued
        working = share - queued
        if span.compile_seconds:
            compiled = working * min(span.compile_seconds / length, 1.0)
            totals[("tool", span.kind, "compile")] += compiled
            totals[("tool", span.kind, "run")] += working - compiled
        else:
            totals[("tool", span.kind, "")] += working
    return dict(totals)


def account(
    db: sqlite3.Connection,
    host: str,
    provider: str,
    transcript: str,
    heavy: frozenset[str] = frozenset(),
) -> int:
    """Writes `window_time` again for a top-level transcript's windows. Returns how many rows
    it wrote."""
    key = (host, provider, transcript)
    db.execute(
        "DELETE FROM window_time WHERE host = ? AND provider = ? AND transcript = ?",
        key,
    )
    found = spans_of(db, host, provider, transcript, heavy)
    written = 0
    for start, end in db.execute(
        "SELECT start, end FROM windows WHERE host = ? AND provider = ? AND transcript = ?",
        key,
    ).fetchall():
        if end is None:
            continue
        for (category, kind, part), value in split(
            found, seconds(str(start)), seconds(str(end))
        ).items():
            db.execute(
                "INSERT INTO window_time (host, provider, transcript, start, category, "
                "kind, part, seconds) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (*key, start, category, kind, part, value),
            )
            written += 1
    return written


def classify(
    db: sqlite3.Connection, table: list[kinds.Kind], where: str, given: tuple[str, ...]
) -> None:
    """Sets the kind and chained flag of the tool calls `where` selects."""
    rows = db.execute(
        f"SELECT rowid, tool, command FROM tool_calls WHERE {where}", given
    ).fetchall()
    db.executemany(
        "UPDATE tool_calls SET kind = ?, chained = ? WHERE rowid = ?",
        [(*kinds.classify(table, str(r[1] or ""), r[2]), int(r[0])) for r in rows],
    )


def refresh(db: sqlite3.Connection, host: str, provider: str, tops: list[str]) -> int:
    """Gives kinds to the tool calls of `host` and `provider` that have none and accounts
    `tops` again. When the command kinds table changed since the stored kinds were given,
    every call is given its kind again and every transcript is accounted again."""
    table, digest = kinds.load()
    digest = f"{VERSION}:{digest}"
    setting = f"command_kinds:{host}:{provider}"
    found = db.execute(
        "SELECT value FROM settings WHERE key = ?", (setting,)
    ).fetchone()
    if found is None or str(found[0]) != digest:
        classify(db, table, "host = ? AND provider = ?", (host, provider))
        tops = [
            str(r[0])
            for r in db.execute(
                "SELECT transcript FROM transcripts WHERE host = ? AND provider = ? "
                "AND parent IS NULL",
                (host, provider),
            )
        ]
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
            (setting, digest),
        )
    else:
        classify(
            db, table, "host = ? AND provider = ? AND kind IS NULL", (host, provider)
        )
    heavy = frozenset(k.kind for k in table if k.heavy)
    return sum(account(db, host, provider, top, heavy) for top in tops)
