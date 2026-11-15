"""`just efficiency metrics [<name>]`: named series of values over time, which trends and the
page draw. A metric is a function from the store, its facts and a scope to points, each a
UTC time and a value. A point is dated by when its unit began: a bead by its start, a run by
its start, a per-day figure by its day. Later parts of the dashboard add their metrics with
`register`. A series carries a mark for each efficiency change made to its metric, which
`changes.py` computes."""

from __future__ import annotations

import sqlite3
from collections import defaultdict
from collections.abc import Callable
from dataclasses import asdict, dataclass

from .facts import Facts, spread, total
from .render import Result, describe, table
from .rollups import day_of
from .scope import Scope

Points = list[tuple[str, float]]


@dataclass(frozen=True)
class Metric:
    name: str
    description: str
    unit: str
    digits: int
    points: Callable[[sqlite3.Connection, Facts, Scope], Points]


METRICS: dict[str, Metric] = {}


def register(metric: Metric) -> Metric:
    if metric.name in METRICS:
        raise ValueError(f"metric {metric.name} is registered twice")
    METRICS[metric.name] = metric
    return metric


def per_bead(
    measure: Callable[[float, float], float],
) -> Callable[[sqlite3.Connection, Facts, Scope], Points]:
    """Points at each bead closed in the scope that has windows in the store, valued by
    `measure(agent seconds, dollars)` over all its windows and dated by when it began.
    """

    def points(_: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
        windows = facts.bead_windows(scope)
        found: Points = []
        for b in facts.closed(scope):
            began = b.began()
            if began and windows.get(b.id):
                t = total(windows[b.id])
                found.append((began, measure(t.agent_seconds, t.dollars)))
        return found

    return points


def context_tokens(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    """Each request's context, for requests in the scope's range, hosts and providers, and
    on the beads its filters choose."""
    beads = facts.cohort(scope)
    found: Points = []
    for time, context, bead, host, provider in db.execute(
        "SELECT time, context_tokens, bead, host, provider FROM requests "
        "WHERE context_tokens IS NOT NULL"
    ):
        if (
            scope.range.holds(str(time))
            and (scope.host is None or host == scope.host)
            and (scope.provider is None or provider == scope.provider)
            and (beads is None or bead in beads)
        ):
            found.append((str(time), float(context)))
    return found


register(
    Metric(
        "bead.dollars",
        "dollars per closed bead",
        "dollars",
        2,
        per_bead(lambda _, dollars: dollars),
    )
)
register(
    Metric(
        "bead.agent-minutes",
        "agent minutes per closed bead",
        "minutes",
        1,
        per_bead(lambda seconds, _: seconds / 60),
    )
)
register(
    Metric(
        "request.context-tokens",
        "tokens of context per model request",
        "tokens",
        0,
        context_tokens,
    )
)


@dataclass
class Day:
    day: str
    count: int
    median: float | None
    p90: float | None


@dataclass
class Mark:
    """An efficiency change on a metric's trend: the moment its commit reached `staging`,
    and the metric's median over the seven days before and after it."""

    label: str
    metric: str
    commit: str | None
    time: str | None
    expect: str
    before: float | None
    after: float | None
    before_count: int
    after_count: int
    verdict: str


@dataclass
class Series:
    metric: str
    description: str
    unit: str
    since: str | None
    range: str
    filters: str
    count: int
    median: float | None
    p90: float | None
    series: list[Day]
    marks: list[Mark]


def listing() -> Result:
    rows = [[m.name, m.unit, m.description] for m in METRICS.values()]
    return Result(
        "metrics",
        {
            "metrics": [
                {"name": m.name, "unit": m.unit, "description": m.description}
                for m in METRICS.values()
            ]
        },
        table(["metric", "unit", "description"], rows),
        [f"efficiency metrics {m}" for m in METRICS][:3],
    )


def number(value: object) -> float | None:
    assert value is None or isinstance(value, (int, float))
    return None if value is None else float(value)


def series(
    db: sqlite3.Connection,
    facts: Facts,
    scope: Scope,
    name: str,
    marks: list[Mark] | None = None,
) -> Result:
    """The metric `name` by day, with `marks`, the efficiency changes made to it."""
    metric = METRICS.get(name)
    if metric is None:
        raise KeyError(f"no metric {name}; `efficiency metrics` lists them")
    points = metric.points(db, facts, scope)
    by_day: dict[str, list[float]] = defaultdict(list)
    for moment, value in points:
        by_day[day_of(moment)].append(value)
    days = []
    for day, values in sorted(by_day.items()):
        found = spread(values, metric.digits)
        days.append(
            Day(day, len(values), number(found["median"]), number(found["p90"]))
        )
    overall = spread([v for _, v in points], metric.digits)
    result = Series(
        name,
        metric.description,
        metric.unit,
        scope.range.since,
        scope.range.name,
        scope.describe(),
        len(points),
        number(overall["median"]),
        number(overall["p90"]),
        days,
        marks or [],
    )
    return Result(
        "metrics",
        asdict(result),
        [
            f"{name}  {metric.description}, in {metric.unit}  since "
            + f"{result.since or 'the start'} ({result.range})"
            + (f"  {result.filters}" if result.filters else ""),
            f"count {result.count}   median {fmt(result.median, metric.digits)}"
            + f"   p90 {fmt(result.p90, metric.digits)}",
            "",
            *table(
                ["day", "count", "median", "p90"],
                [[d.day, d.count, d.median, d.p90] for d in days],
            ),
            *mark_lines(result.marks, metric.digits),
        ],
        ["efficiency metrics", "efficiency changes"],
        describe("since", "series", "marks"),
    )


def fmt(value: float | None, digits: int) -> str:
    return "-" if value is None else f"{value:.{digits}f}"


def mark_lines(marks: list[Mark], digits: int) -> list[str]:
    if not marks:
        return []
    return [
        "",
        "efficiency changes",
        *table(
            ["reached staging", "change", "before", "after", "verdict"],
            [
                [
                    day_of(m.time) if m.time else "-",
                    m.label,
                    f"{fmt(m.before, digits)} ({m.before_count})",
                    f"{fmt(m.after, digits)} ({m.after_count})",
                    m.verdict,
                ]
                for m in marks
            ],
        ),
    ]
