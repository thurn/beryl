"""Results and their text rendering. A query returns a `Result`: plain data for `--json` and
the page, the lines the command prints, the queries that drill into it, and a description of
each field, which the page shows on hover. The lines only format numbers the data holds, so
the text and the JSON cannot disagree."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field


@dataclass
class Result:
    query: str
    data: dict[str, object]
    lines: list[str] = field(default_factory=list)
    more: list[str] = field(default_factory=list)
    fields: dict[str, str] = field(default_factory=dict)

    def json(self) -> dict[str, object]:
        found: dict[str, object] = {"query": self.query, **self.data, "more": self.more}
        if self.fields:
            found["fields"] = self.fields
        return found


# What each field of a query's result means, by name.
FIELDS: dict[str, str] = {
    "since": "the start of the range, in UTC; null for no start. A window counts when it "
    "starts in the range, a request or tool call when it happened in it, and a bead when "
    "it closed in it",
    "beads_closed": "beads, not epics, closed in the range",
    "per_day": "beads closed per day of the range",
    "no_data": "closed beads with no session window in the store, such as beads that ran "
    "on another host",
    "agent_minutes": "wall-clock minutes a session worked: running tools, generating, "
    "queued for cores or waiting on background work. Per bead: the median over closed "
    "beads, with the 90th percentile and the median over the range before",
    "core_minutes": "CPU minutes used in the bead's worktree; not measured yet",
    "dollars": "tokens times each model's published API price. Per bead: the median over "
    "closed beads, with the 90th percentile and the median over the range before",
    "rework": "dollars of later fixes the bead's commits caused; not measured yet",
    "person_minutes": "minutes waiting on a person, beside agent minutes",
    "idle_minutes": "minutes idle after a turn ended, such as before a scheduled wake-up, "
    "beside agent minutes",
    "time": "agent minutes by what the session was doing",
    "beside": "minutes beside agent minutes: waiting on a person, and idle",
    "dollars_by_source": "dollars by where they came from: output, the fixed prefix, and "
    "new and carried context by what added it",
    "lines": "the roll-up's lines, which sum to its total; orchestration is an "
    "orchestrator session's cost charged to its epic, and (no bead) is work in no bead",
    "lower_bound": "true when a model with no price makes the dollars a lower bound; the "
    "text marks such a number with +",
    "unpriced": "models with no price, with their tokens",
    "percent": "share of the total, in percent",
    "tokens": "tokens of every class",
    "requests": "model requests",
    "context_tokens": "tokens of context a request read",
    "prefix_tokens": "the context of the transcript's first request, carried by every "
    "later one",
    "rows": "the session's requests and tool calls in order",
    "series": "the metric's values by day: count, median and 90th percentile",
}


def describe(*names: str) -> dict[str, str]:
    return {n: FIELDS[n] for n in names}


def mark(lower_bound: bool) -> str:
    """The mark after a sum that a model with no price makes a lower bound."""
    return "+" if lower_bound else ""


def cell(value: object) -> str:
    """A value as a table shows it: unknown is a dash, never zero."""
    if value is None:
        return "-"
    if isinstance(value, float):
        return f"{value:.2f}"
    return str(value)


def table(headers: Sequence[str], rows: Sequence[Sequence[object]]) -> list[str]:
    """`rows` under `headers` in columns two spaces apart, numbers right-aligned."""
    shown = [[cell(v) for v in row] for row in rows]
    widths: list[int] = [
        max([len(h), *(len(r[i]) for r in shown)]) for i, h in enumerate(headers)
    ]
    numeric: list[bool] = [
        bool(rows) and all(isinstance(r[i], (int, float)) or r[i] is None for r in rows)
        for i in range(len(headers))
    ]

    def line(values: Sequence[str]) -> str:
        return "  ".join(
            v.rjust(w) if n else v.ljust(w) for v, w, n in zip(values, widths, numeric)
        ).rstrip()

    return [line(list(headers)), *(line(r) for r in shown)]


def text(result: Result) -> str:
    lines = list(result.lines)
    if result.more:
        lines.append("")
        lines.append("more: " + " | ".join(result.more))
    return "\n".join(lines)
