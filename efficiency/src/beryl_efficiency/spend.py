"""`efficiency spend`: each ticket window of this host as segments ready to draw, for the
efficiency section of Ignite's ticket pages (docs/efficiency-dashboard.md, "Ticket pages on
the hub").

Each window becomes one JSON line with two lists in time order:

- `time`: the window's seconds, by the category and kind that won each stretch of the
  sweep in `timing` (see "Time accounting"). A tool call's stretch is split into its queued
  seconds first, then its compile seconds, then the rest, since a command waits for cores
  before it builds and builds before it runs. Neighbouring stretches with the same category,
  kind and part are one segment.
- `dollars`: the window's requests, each under the kind of the tool calls it made, or
  `reply` for a request that made none. Neighbouring requests of the same kind are one
  segment.

A window with many segments is coalesced to at most `MAX_SEGMENTS` in each list, because a
segment the height of a pixel cannot be hovered. A coalesced segment takes the key of its
largest part and lists its largest parts in `mix`. Waits on a person and idle stretches are
never merged with anything: the page draws each one as a break of its own.

Each segment also names its group in `g`, one of `GROUPS`, which decides its colour on the page:
the hub knows the groups and nothing about this project's command kinds.

A segment carries the command that took most of its seconds in `x`. The hub shows it only on
its private listener, since commands hold paths.
"""

from __future__ import annotations

import bisect
import json
import sqlite3
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

from . import kinds, timing

# The most segments one window has in each list.
MAX_SEGMENTS = 120
# How many of its parts a coalesced segment lists.
MIX_SIZE = 3
# The longest command a segment carries.
COMMAND_CHARS = 200
# The categories the page draws as breaks rather than by their length.
BREAKS: frozenset[str] = frozenset({"person", "idle"})
# The kind of a request that made no tool call: the model answering or ending its turn.
REPLY = "reply"
# The colour groups, in the order the page's legend lists them. `person` and `idle` are the
# breaks; the page draws them apart from the rest.
GROUPS: tuple[str, ...] = (
    "model",
    "checks",
    "tools",
    "ignite",
    "background",
    "agents",
    "queued",
    "person",
    "idle",
)
# Kinds that hand work to Ignite or wait on it, besides every kind ending in "wait".
IGNITE_KINDS: frozenset[str] = frozenset(
    {"ignite", "ignite start", "push", "enact", "enact sync", "bd"}
)
# Tools that start or reach other agents, besides every other MCP tool.
AGENT_KINDS: frozenset[str] = frozenset(
    {"Agent", "Task", "ToolSearch", "Skill", "WebFetch", "WebSearch"}
)
# The token classes, in the order the `dollars` segments list them.
CLASSES: tuple[str, ...] = ("input", "cache_write", "cache_read", "output")


def hub_id(bead: str) -> str:
    """The hub's spelling of a stored ticket key: `by-mp4b.12` is `TMP4B.12`. The store keeps
    the lowercase keys from before Ignite (see `ticket_ids`)."""
    return "T" + bead.removeprefix("by-").upper()


def group_of(category: str, kind: str, heavy: frozenset[str]) -> str:
    """The colour group of a segment: its category when that is not a tool, then whether its
    kind waits on Ignite, runs checks or builds (a heavy kind), reaches other agents, or is any
    other tool."""
    if category != "tool":
        return category
    if kind in IGNITE_KINDS or kind.endswith("wait") or kind.startswith("mcp__ccd_"):
        return "ignite"
    if kind in heavy:
        return "checks"
    if kind in AGENT_KINDS or kind.startswith("mcp__"):
        return "agents"
    return "tools"


@dataclass
class Segment:
    """One run of a list. `weight` is what the run is sized by: seconds or dollars."""

    category: str
    kind: str
    part: str
    at: float  # seconds from the window's start
    weight: float = 0.0
    calls: set[str] = field(default_factory=set)
    by_call: dict[str, float] = field(default_factory=lambda: defaultdict(float))
    requests: int = 0
    tokens: list[float] = field(default_factory=lambda: [0.0] * len(CLASSES))
    class_dollars: list[float] = field(default_factory=lambda: [0.0] * len(CLASSES))
    models: dict[str, float] = field(default_factory=lambda: defaultdict(float))
    mix: dict[str, float] = field(default_factory=lambda: defaultdict(float))

    def key(self) -> tuple[str, str, str]:
        return (self.category, self.kind, self.part)

    def label(self) -> str:
        return " ".join(p for p in (self.kind or self.category, self.part) if p)

    def absorb(self, other: Segment, mixing: bool = False) -> None:
        """Adds `other` to this segment. When `mixing`, as in coalescing, `other` is also
        listed among its parts."""
        self.weight += other.weight
        self.calls |= other.calls
        for call, value in other.by_call.items():
            self.by_call[call] += value
        self.requests += other.requests
        for index in range(len(CLASSES)):
            self.tokens[index] += other.tokens[index]
            self.class_dollars[index] += other.class_dollars[index]
        for model, value in other.models.items():
            self.models[model] += value
        if not mixing:
            return
        if other.mix:
            for name, value in other.mix.items():
                self.mix[name] += value
        else:
            self.mix[other.label()] += other.weight


def merged(segments: list[Segment]) -> list[Segment]:
    """Neighbouring segments with the same key as one."""
    found: list[Segment] = []
    for segment in segments:
        if found and found[-1].key() == segment.key():
            found[-1].absorb(segment)
        else:
            found.append(segment)
    return found


def coalesced(segments: list[Segment], limit: int = MAX_SEGMENTS) -> list[Segment]:
    """At most about `limit` segments: runs between breaks are gathered into buckets of at
    least 1/`limit` of the total weight, each under the key of its largest part."""
    if len(segments) <= limit:
        return segments
    total = sum(s.weight for s in segments if s.category not in BREAKS)
    target = total / limit
    found: list[Segment] = []
    bucket: list[Segment] = []

    def close() -> None:
        if not bucket:
            return
        if len(bucket) == 1:
            found.append(bucket[0])
        else:
            parts: dict[tuple[str, str, str], float] = defaultdict(float)
            for s in bucket:
                parts[s.key()] += s.weight
            category, kind, part = max(parts.items(), key=lambda item: item[1])[0]
            head = Segment(category, kind, part, bucket[0].at)
            for s in bucket:
                head.absorb(s, mixing=True)
            found.append(head)
        bucket.clear()

    for segment in segments:
        if segment.category in BREAKS:
            close()
            found.append(segment)
            continue
        bucket.append(segment)
        if sum(s.weight for s in bucket) >= target:
            close()
    close()
    return found


def time_segments(spans: list[timing.Span], start: float, end: float) -> list[Segment]:
    """The window from `start` to `end` as time segments, in order."""
    found: list[Segment] = []
    for low, high, winner, running in timing.sweep(spans, start, end):
        if winner == "model":
            found.append(Segment("model", "", "", low - start, high - low))
            continue
        if winner != "tool":
            share = (high - low) / len(running)
            for span in running:
                s = Segment(winner, span.kind, "", low - start, share)
                call = span.call
                if call:
                    s.calls.add(call)
                    s.by_call[call] += share
                found.append(s)
            continue
        # The earliest call running decides the stretch's kind, as the page shows one
        # colour at a time; every call running counts towards its calls.
        lead = min(running, key=lambda span: span.start)
        queued_until = lead.start + lead.queued_seconds
        compiled_until = queued_until + lead.compile_seconds
        edges = sorted(
            {low, high, *(t for t in (queued_until, compiled_until) if low < t < high)}
        )
        for left, right in zip(edges, edges[1:]):
            if right <= queued_until:
                category, part = "queued", ""
            elif lead.compile_seconds and right <= compiled_until:
                category, part = "tool", "compile"
            else:
                category, part = "tool", "run" if lead.compile_seconds else ""
            s = Segment(category, lead.kind, part, left - start, right - left)
            for span in running:
                call = span.call
                if call:
                    s.calls.add(call)
                    s.by_call[call] += (right - left) / len(running)
            found.append(s)
    return merged(found)


@dataclass
class Request:
    time: str
    model: str
    tokens: list[float]
    class_dollars: list[float]


def dollar_segments(
    db: sqlite3.Connection,
    key: tuple[str, str, str],
    start: str,
    end: str,
    calls: list[tuple[str, str, str]],
) -> list[Segment]:
    """The requests of a window, in order, each under the kind of the calls it made.
    `calls` is the transcript's (call, start, kind), sorted by start."""
    requests = [
        Request(
            str(r[0]),
            str(r[1] or ""),
            [float(v or 0) for v in r[2:6]],
            [float(v or 0) for v in r[6:10]],
        )
        for r in db.execute(
            """SELECT time, model, input_tokens, cache_write_tokens, cache_read_tokens,
                output_tokens, input_dollars, cache_write_dollars, cache_read_dollars,
                output_dollars
            FROM requests WHERE host = ? AND provider = ? AND transcript = ?
            ORDER BY time, message""",
            key,
        )
    ]
    times = [r.time for r in requests]
    made: dict[int, list[str]] = defaultdict(list)
    for call, call_start, kind in calls:
        # A call belongs to the request whose message holds it: the first at or after it
        # (as in `tokens`).
        owner = bisect.bisect_left(times, call_start)
        if owner < len(requests):
            made[owner].append(kind)
    origin = timing.seconds(start)
    found: list[Segment] = []
    for index, r in enumerate(requests):
        if not start <= r.time <= end:
            continue
        named = Counter(made.get(index, [])).most_common(1)
        kind = named[0][0] if named else REPLY
        s = Segment(
            "model" if kind == REPLY else "tool",
            kind,
            "",
            timing.seconds(r.time) - origin,
            sum(r.class_dollars),
        )
        s.requests = 1
        s.tokens = list(r.tokens)
        s.class_dollars = list(r.class_dollars)
        if r.model:
            s.models[r.model] += s.weight
        found.append(s)
    return merged(found)


def encoded(
    segment: Segment, commands: dict[str, str], dollars: bool, heavy: frozenset[str]
) -> dict[str, object]:
    """One segment as the hub stores it. Keys are short because a large epic has thousands."""
    found: dict[str, object] = {
        "c": segment.category,
        "k": segment.kind,
        "g": group_of(segment.category, segment.kind, heavy),
        "t": round(segment.at, 1),
    }
    if segment.part:
        found["p"] = segment.part
    if dollars:
        found["d"] = round(segment.weight, 5)
        found["r"] = segment.requests
        found["tk"] = [round(v) for v in segment.tokens]
        found["cd"] = [round(v, 5) for v in segment.class_dollars]
        if segment.models:
            found["m"] = max(segment.models.items(), key=lambda item: item[1])[0]
    else:
        found["s"] = round(segment.weight, 2)
        if segment.calls:
            found["n"] = len(segment.calls)
        if segment.by_call:
            call = max(segment.by_call.items(), key=lambda item: item[1])[0]
            command = commands.get(call)
            if command:
                found["x"] = command[:COMMAND_CHARS]
    if segment.mix:
        found["mix"] = [
            [name, round(value, 5 if dollars else 2)]
            for name, value in sorted(segment.mix.items(), key=lambda item: -item[1])[
                :MIX_SIZE
            ]
        ]
    return found


def windows(
    db: sqlite3.Connection, host: str, since: str | None
) -> list[dict[str, object]]:
    """Every ticket window of `host` that ended at or after `since` (all when None)."""
    table, _ = kinds.load()
    heavy = frozenset(k.kind for k in table if k.heavy)
    rows = db.execute(
        """SELECT provider, session, transcript, start, end, bead FROM windows
        WHERE host = ? AND kind = 'bead' AND bead IS NOT NULL AND end IS NOT NULL
            AND end >= ?
        ORDER BY provider, transcript, start""",
        (host, since or ""),
    ).fetchall()
    found: list[dict[str, object]] = []
    loaded: tuple[str, str] | None = None
    spans: list[timing.Span] = []
    calls: list[tuple[str, str, str]] = []
    commands: dict[str, str] = {}
    for provider, session, transcript, start, end, bead in rows:
        key = (host, str(provider), str(transcript))
        if loaded != key[1:]:
            loaded = (str(provider), str(transcript))
            spans = timing.spans_of(db, *key, heavy)
            listed = db.execute(
                """SELECT call, start, coalesce(kind, ?), command FROM tool_calls
                WHERE host = ? AND provider = ? AND transcript = ? ORDER BY start""",
                (kinds.OTHER, *key),
            ).fetchall()
            calls = [(str(c[0]), str(c[1]), str(c[2])) for c in listed]
            commands = {str(c[0]): str(c[3]) for c in listed if c[3]}
        low, high = timing.seconds(str(start)), timing.seconds(str(end))
        time = coalesced(time_segments(spans, low, high))
        dollars = coalesced(dollar_segments(db, key, str(start), str(end), calls))
        found.append(
            {
                "ticket": hub_id(str(bead)),
                "window": f"{provider}:{transcript}:{start}",
                "provider": str(provider),
                "session": str(session),
                "start": str(start),
                "end": str(end),
                "time": [encoded(s, commands, False, heavy) for s in time],
                "dollars": [encoded(s, commands, True, heavy) for s in dollars],
            }
        )
    return found


def write(found: list[dict[str, object]], out: Path) -> None:
    """One window per line."""
    with out.open("w", encoding="utf-8") as file:
        for window in found:
            file.write(json.dumps(window, separators=(",", ":")) + "\n")
