"""Results and their text rendering. A query returns a `Result`: plain data for `--json` and
the page, the lines the command prints, the queries that drill into it, and a description of
each field, which the page shows on hover. The lines only format numbers the data holds, so
the text and the JSON cannot disagree."""

from __future__ import annotations

import re
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
    "core_minutes": "CPU minutes used in the bead's worktree while the bead was open, from "
    "the CPU sampler; null where the sampler was not running. Per bead: the median over "
    "closed beads the sampler measured, with the 90th percentile and the median before",
    "dollars": "tokens times each model's published API price. Per bead: the median over "
    "closed beads, with the 90th percentile and the median over the range before",
    "rework": "dollars of later fixes the bead's commits caused: a share of a Steward "
    "bead's cost for a slow-CI failure in a range with its commits, and a share of a later "
    "bead's cost for `fix` commits to card files it added. Part of the fixing bead's direct "
    "dollars, so never added to a total. Per bead: the median over closed beads",
    "rework_percent": "rework as a percent of the bead's direct dollars",
    "rework_from": "the beads whose direct dollars the rework is part of, and by which rule",
    "commits": "commits the bead landed, from its enact_commits",
    "cards": "card files under rust/cards/ that the bead's commits added first",
    "queue_minutes": "minutes from the bead's creation to its start",
    "work_minutes": "minutes from the bead's start to its close",
    "to_master_minutes": "minutes from the bead's last commit on staging to that commit "
    "reaching master, from master's reflog",
    "lead_minutes": "minutes from the bead's creation to its last commit reaching master",
    "fixed_later": "percent of the card files that a later fix commit by another bead "
    "changed",
    "per_card": "direct dollars, rework dollars and agent minutes over the card files "
    "added",
    "cores": "the machine's cores, from the CPU sampler",
    "cores_in_use": "the cores in use on average over the sampled minutes, a minute "
    "sampled over the machine's cores counting as all of them",
    "over_cores": "sampled minutes whose CPU seconds exceed what the machine's cores can "
    "do in a minute, which the sampler overcounted",
    "peak_cores": "the cores in use on average in the busiest sampled minute. The "
    "sampler's own peak between two samples is not used, because it can exceed the "
    "machine's cores",
    "saturation": "percent of the sampled minutes in which the cores in use were at least "
    "90% of the machine's",
    "queued_hours": "hours commands waited for CPU tokens, from the token pool's leases",
    "queued_core_hours": "core hours held up waiting for CPU tokens: each lease's wait "
    "times the tokens its command wanted",
    "queued": "leases by class and command: how many, the hours they waited and the core "
    "hours that held up",
    "narrow": "leases that got fewer CPU tokens than they wanted, by class and command",
    "core_hours": "CPU hours the sampler saw in the range",
    "bead_core_hours": "CPU hours in beads' worktrees that belong to the beads",
    "shared_core_hours": "CPU hours no bead claims, per day sampled and per bead closed",
    "shared": "CPU hours no bead claims, by owner and kind of work, or by worktree for a "
    "worktree's minutes outside its bead",
    "own": "the tool's own CPU: the sampler's cores on average since it started and the "
    "range's ingests' CPU seconds, each also as a percent of the machine",
    "capacity": "the cohort's cores per session (core minutes over agent minutes) and how "
    "many such sessions the machine's cores could run at once",
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


# A number as text, maybe marked as a lower bound: a table aligns it as a number.
MARKED: re.Pattern[str] = re.compile(r"^-?\d+(\.\d+)?\+?$")
# How wide a line of split parts may grow before the next part starts a new line.
WIDTH = 100


def wrap(head: str, parts: Sequence[str], indent: int = 10) -> list[str]:
    """`head`, then `parts` joined by bars, broken into lines of at most `WIDTH` characters
    whose continuations are indented by `indent`."""
    lines = [head]
    for part in parts:
        joined = f"{lines[-1]} | {part}" if lines[-1] != head else f"{head}{part}"
        if len(joined) > WIDTH and lines[-1] != head:
            lines.append(" " * indent + part)
        else:
            lines[-1] = joined
    return lines


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
        bool(rows)
        and all(
            isinstance(r[i], (int, float))
            or r[i] is None
            or (isinstance(r[i], str) and MARKED.match(str(r[i])) is not None)
            for r in rows
        )
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
