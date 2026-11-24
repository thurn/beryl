"""The series Ignite is built to move and the three that keep its gate honest (docs/ignite.md,
"What Ignite must achieve" and "Records and metrics"), from the `ignite_*` tables that
`ignite.py` copies from the hub. Each takes the usual filters: `--host` keeps one machine's
records, the ticket filters (labels, epic, role, tier) keep their tickets, and `--provider`
applies to dispatch cost, the one series made of model requests.

- `ignite.cycle-minutes`: minutes from each ticket's claim to its landing.
- `ignite.worker-wait-percent`: of each landed ticket's lead time, from its creation to its
  landing, the percent before a worker claimed it. The hub keeps no history of when a ticket
  became ready, so time spent blocked counts as waiting.
- `ignite.repair-percent`: each day, the percent of landed tickets that were repairs (the
  ticket of a regression, or one handed off as a regression).
- `ignite.master-lag-minutes`: minutes from the hub first seeing each tip to its being
  verified, which is how long the release branch stays behind it.
- `ignite.local-tip-round-minutes`: the wall time of each finished verification round
  recorded by the local daemon.
- `ignite.dispatch-dollars`: each day, the dollars of the dispatcher sessions' requests.
- `ignite.escapes-per-100-landings`: each day, regressions handed off per 100 landings.
- `ignite.cache-hit-percent`: each day, of the gate's and tip verification's check families,
  the percent the result cache answered (records with the verb `cached`).
- `ignite.gate-instructions`, `ignite.gate-seconds`, `ignite.gate-wait-seconds`: each landing's
  gate (the `land` records of one ticket and commit), in billions of instructions, seconds
  run, and seconds waiting for cores apart.
"""

from __future__ import annotations

import sqlite3
from collections import defaultdict
from collections.abc import Callable
from datetime import datetime

from . import ticket_ids
from .facts import Facts
from .metrics import Metric, Points, register
from .rollups import day_of
from .scope import Scope


def minutes_between(start: str, end: str) -> float:
    def parse(text: str) -> datetime:
        return datetime.fromisoformat(text.replace("Z", "+00:00"))

    return (parse(end) - parse(start)).total_seconds() / 60


def landed(db: sqlite3.Connection, facts: Facts, scope: Scope) -> list[tuple[str, ...]]:
    """`(id, created, claimed, landed, repair)` of each ticket claimed and landed, landed in
    the range, and chosen by the ticket filters. The hub's IDs carry no project prefix, so
    each is resolved as the store spells it before it is compared."""
    chosen = facts.cohort(scope)
    return [
        (str(i), str(c), str(cl), str(la), str(rep))
        for i, c, cl, la, rep in db.execute(
            "SELECT id, created_at, claimed_at, landed_at, repair FROM ignite_tickets "
            "WHERE claimed_at IS NOT NULL AND landed_at IS NOT NULL ORDER BY landed_at"
        )
        if scope.range.holds(str(la))
        and (chosen is None or ticket_ids.stored(db, str(i)) in chosen)
    ]


def per_day(found: dict[str, list[str]], value: dict[str, float]) -> Points:
    """A point per day, dated by the day's first moment in `found`."""
    return [(min(found[day]), value[day]) for day in sorted(found)]


def cycle_minutes(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    return [
        (cl, minutes_between(cl, la)) for _, _, cl, la, _ in landed(db, facts, scope)
    ]


def worker_wait(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    found: Points = []
    for _, created, claimed, done, _ in landed(db, facts, scope):
        lead = minutes_between(created, done)
        if lead > 0:
            found.append((created, 100 * minutes_between(created, claimed) / lead))
    return found


def repair_percent(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    days: dict[str, list[str]] = defaultdict(list)
    repairs: dict[str, float] = defaultdict(float)
    for _, _, _, done, repair in landed(db, facts, scope):
        days[day_of(done)].append(done)
        repairs[day_of(done)] += float(repair)
    return per_day(days, {d: 100 * repairs[d] / len(days[d]) for d in days})


def master_lag(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    return [
        (str(seen), minutes_between(str(seen), str(verified)))
        for seen, verified in db.execute(
            "SELECT seen, verified FROM ignite_tips WHERE verified IS NOT NULL "
            "ORDER BY seen"
        )
        if scope.range.holds(str(seen))
    ]


def local_tip_round_minutes(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    return [
        (str(start), minutes_between(str(start), str(end)))
        for start, end, host in db.execute(
            "SELECT started_at, finished_at, host FROM ignite_tip_rounds ORDER BY started_at"
        )
        if (scope.host is None or scope.host == host) and scope.range.holds(str(start))
    ]


def dispatch_dollars(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    days: dict[str, list[str]] = defaultdict(list)
    dollars: dict[str, float] = defaultdict(float)
    for time, spent, host, provider in db.execute(
        """SELECT r.time, r.dollars, r.host, r.provider FROM requests AS r
        JOIN sessions AS s ON s.host = r.host AND s.provider = r.provider
            AND s.transcript = r.transcript
        WHERE r.dollars IS NOT NULL AND s.session IN
            (SELECT session FROM ignite_workers WHERE role = 'dispatcher')"""
    ):
        if (
            scope.range.holds(str(time))
            and (scope.host is None or host == scope.host)
            and (scope.provider is None or provider == scope.provider)
        ):
            days[day_of(str(time))].append(str(time))
            dollars[day_of(str(time))] += float(spent)
    return per_day(days, dollars)


def escapes(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    days: dict[str, list[str]] = defaultdict(list)
    for _, _, _, done, _ in landed(db, facts, scope):
        days[day_of(done)].append(done)
    found: dict[str, float] = defaultdict(float)
    for (at,) in db.execute("SELECT at FROM ignite_handoffs WHERE kind = 'regression'"):
        if day_of(str(at)) in days:
            found[day_of(str(at))] += 1
    return per_day(days, {d: 100 * found[d] / len(days[d]) for d in days})


RECORD_COLUMNS = (
    "host",
    "ticket",
    "commit",
    "verb",
    "at",
    "instructions",
    "wall_seconds",
    "queued_seconds",
)


def records(
    db: sqlite3.Connection, facts: Facts, scope: Scope, where: str
) -> list[dict[str, object]]:
    """The records in the range, on the scope's host and chosen tickets, that `where`
    keeps."""
    chosen = facts.cohort(scope)
    columns = ", ".join(f'"{c}"' for c in RECORD_COLUMNS)
    found = [
        dict(zip(RECORD_COLUMNS, values))
        for values in db.execute(
            f"SELECT {columns} FROM ignite_records WHERE {where} ORDER BY at"
        )
    ]
    return [
        r
        for r in found
        if scope.range.holds(str(r["at"]))
        and (scope.host is None or r["host"] == scope.host)
        and (chosen is None or r["ticket"] in chosen)
    ]


def cache_hits(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    days: dict[str, list[str]] = defaultdict(list)
    hits: dict[str, float] = defaultdict(float)
    where = "class IN ('land', 'tip') AND family IS NOT NULL"
    for r in records(db, facts, scope, where):
        at = str(r["at"])
        days[day_of(at)].append(at)
        hits[day_of(at)] += float(r["verb"] == "cached")
    return per_day(days, {d: 100 * hits[d] / len(days[d]) for d in days})


def gates(db: sqlite3.Connection, facts: Facts, scope: Scope) -> list[list[object]]:
    """Per landing, the gate's first record's end, instructions, seconds and wait."""
    found: dict[tuple[str, str, str], list[object]] = {}
    for r in records(db, facts, scope, "class = 'land' AND verb != 'cached'"):
        key = (str(r["host"]), str(r["ticket"]), str(r["commit"]))
        gate = found.setdefault(key, [str(r["at"]), 0.0, 0.0, 0.0])
        gate[1] = float(str(gate[1])) + float(str(r["instructions"] or 0)) / 1e9
        gate[2] = float(str(gate[2])) + float(str(r["wall_seconds"]))
        gate[3] = float(str(gate[3])) + float(str(r["queued_seconds"]))
    return sorted(found.values(), key=lambda g: str(g[0]))


def gate_part(index: int) -> Callable[[sqlite3.Connection, Facts, Scope], Points]:
    def points(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
        return [(str(g[0]), float(str(g[index]))) for g in gates(db, facts, scope)]

    return points


for metric in (
    Metric(
        "ignite.cycle-minutes",
        "minutes from each ticket's claim to its landing",
        "minutes",
        0,
        cycle_minutes,
    ),
    Metric(
        "ignite.worker-wait-percent",
        "percent of each landed ticket's lead time before a worker claimed it",
        "percent",
        1,
        worker_wait,
    ),
    Metric(
        "ignite.repair-percent",
        "percent of each day's landed tickets that were repairs",
        "percent",
        1,
        repair_percent,
    ),
    Metric(
        "ignite.master-lag-minutes",
        "minutes from the hub seeing each tip to its verification",
        "minutes",
        0,
        master_lag,
    ),
    Metric(
        "ignite.local-tip-round-minutes",
        "minutes a finished tip verification round took on the local daemon",
        "minutes",
        0,
        local_tip_round_minutes,
    ),
    Metric(
        "ignite.dispatch-dollars",
        "dollars of the dispatcher sessions each day",
        "dollars",
        2,
        dispatch_dollars,
    ),
    Metric(
        "ignite.escapes-per-100-landings",
        "regressions handed off per 100 landings each day",
        "escapes",
        1,
        escapes,
    ),
    Metric(
        "ignite.cache-hit-percent",
        "percent of each day's gate and tip check families the result cache answered",
        "percent",
        1,
        cache_hits,
    ),
    Metric(
        "ignite.gate-instructions",
        "billions of instructions of each landing's gate",
        "billions",
        1,
        gate_part(1),
    ),
    Metric(
        "ignite.gate-seconds",
        "seconds each landing's gate ran, its wait for cores apart",
        "seconds",
        0,
        gate_part(2),
    ),
    Metric(
        "ignite.gate-wait-seconds",
        "seconds each landing's gate waited for cores",
        "seconds",
        0,
        gate_part(3),
    ),
):
    register(metric)
