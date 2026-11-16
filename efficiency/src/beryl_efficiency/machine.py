"""`just efficiency machine`: how busy the machine was and where its CPU went
(docs/efficiency-dashboard.md, "CPU accounting"): cores in use, saturation, the time commands
queued for CPU tokens, leases narrower than they wanted, the CPU no bead claims, the tool's
own cost, and how many sessions of the cohort the machine could run at once.

Its metrics are `machine.saturation`, `machine.queued-core-hours-per-day` and
`bead.core-minutes`.
"""

from __future__ import annotations

import math
import sqlite3
from collections import defaultdict
from dataclasses import asdict, dataclass

from . import cpu
from .facts import Bead, Facts, spread, total
from .metrics import Metric, Points, register
from .render import Result, describe, table
from .rollups import Part, bar, day_of, number, split_lines
from .scope import Scope

# A minute is saturated when the cores in use were at least this share of the machine's.
SATURATED = 0.9


@dataclass
class Queued:
    """The leases of one class and command: how many, the hours they waited, and the core
    hours that waiting held up (each wait times the tokens the command wanted)."""

    klass: str
    command: str
    leases: int
    waited_hours: float
    core_hours: float


@dataclass
class Narrow:
    """Leases of one class and command that got fewer tokens than they wanted."""

    klass: str
    command: str
    leases: int
    narrower: int
    mean_cores: float
    mean_wanted: float


@dataclass
class Own:
    """The tool's own CPU: the sampler's cores on average since it started, and the CPU
    seconds of the range's ingests; each also as a percent of the machine's cores."""

    sampler_cores: float | None
    sampler_percent: float | None
    ingest_cpu_seconds: float
    ingest_percent: float | None


@dataclass
class Capacity:
    """The cohort's closed beads with both core and agent minutes: their cores per session
    (core minutes over agent minutes), and the machine's cores over the mean, rounded down.
    """

    beads: int
    mean_cores: float | None
    p90_cores: float | None
    machine_cores: int | None
    sessions: int | None


@dataclass
class Machine:
    since: str | None
    until: str | None
    range: str
    filters: str
    minutes: int
    cores: int | None
    cores_in_use: float | None
    peak_cores: float | None
    saturation: float | None
    over_cores: int
    queued_hours: float
    queued_core_hours: float
    queued: list[Queued]
    narrow: list[Narrow]
    core_hours: float
    bead_core_hours: float
    shared_core_hours: float
    shared_per_day: float | None
    shared_per_bead: float | None
    shared: list[Part]
    own: Own
    capacity: Capacity


def bounds(column: str, scope: Scope) -> tuple[str, list[object]]:
    """A `WHERE` clause (starting with `AND`) that keeps `column` in the scope's range and
    the rows on its host."""
    clause = ""
    given: list[object] = []
    if scope.range.since:
        clause += f" AND {column} >= ?"
        given.append(scope.range.since)
    if scope.range.until:
        clause += f" AND {column} < ?"
        given.append(scope.range.until)
    if scope.host:
        clause += " AND host = ?"
        given.append(scope.host)
    return clause, given


def hours(seconds: float) -> float:
    return round(seconds / 3600, 2)


def queued(db: sqlite3.Connection, scope: Scope) -> tuple[list[Queued], list[Narrow]]:
    clause, given = bounds("time", scope)
    waits = [
        Queued(str(r[0]), str(r[1]), int(r[2]), hours(float(r[3])), hours(float(r[4])))
        for r in db.execute(
            f"""SELECT class, command, count(*), sum(waited_seconds),
                sum(waited_seconds * wanted) AS held_up
            FROM leases WHERE 1{clause} GROUP BY class, command
            ORDER BY held_up DESC, class, command""",
            given,
        )
    ]
    narrow = [
        Narrow(
            str(r[0]),
            str(r[1]),
            int(r[2]),
            int(r[3]),
            round(float(r[4]), 1),
            round(float(r[5]), 1),
        )
        for r in db.execute(
            f"""SELECT class, command, count(*), sum(cores < wanted) AS fewer, avg(cores),
                avg(wanted)
            FROM leases WHERE cores > 0{clause} GROUP BY class, command HAVING fewer > 0
            ORDER BY fewer DESC, class, command""",
            given,
        )
    ]
    return waits, narrow


def shared_cpu(
    db: sqlite3.Connection, facts: Facts, scope: Scope
) -> tuple[float, dict[str, float]]:
    """The range's CPU seconds that beads claim, and the rest by owner and kind, or by
    worktree for a worktree's minutes that no bead claims."""
    clause, given = bounds("time", scope)
    shared: dict[str, float] = defaultdict(float)
    for owner, kind, seconds in db.execute(
        f"SELECT owner, kind, sum(cpu_seconds) FROM cpu WHERE worktree = ''{clause} "
        "GROUP BY owner, kind",
        given,
    ):
        shared[f"{owner}/{kind}"] += float(seconds)
    claimed = 0.0
    worktrees = [
        str(r[0])
        for r in db.execute(
            f"SELECT DISTINCT worktree FROM cpu WHERE worktree != ''{clause}", given
        )
    ]
    for name in worktrees:
        minutes = cpu.worktree_minutes(
            db, name, scope.range.since, scope.range.until, scope.host
        )
        by_bead, rest = cpu.attribute(minutes, facts.claims(name))
        claimed += sum(by_bead.values())
        if rest:
            shared[f"worktree {name}"] += rest
    return claimed, dict(shared)


def machine_cores(db: sqlite3.Connection) -> int | None:
    """The most cores the sampler has seen the machine have."""
    found = db.execute("SELECT max(cores) FROM cpu_minutes").fetchone()[0]
    return int(found) if found else None


def capacity(
    facts: Facts, scope: Scope, cores: int | None, beads: list[Bead] | None = None
) -> Capacity:
    """The capacity estimate over `beads`, by default the beads closed in the range."""
    windows = facts.bead_windows(scope)
    per_session: list[float] = []
    for b in facts.closed(scope) if beads is None else beads:
        core = facts.core_minutes(b.id)
        agent = total(windows.get(b.id, [])).agent_seconds / 60
        if core is not None and agent > 0:
            per_session.append(core / agent)
    if not per_session:
        return Capacity(0, None, None, cores, None)
    mean = sum(per_session) / len(per_session)
    p90 = spread(per_session, 1)["p90"]
    assert isinstance(p90, float)
    return Capacity(
        len(per_session),
        round(mean, 1),
        p90,
        cores,
        math.floor(cores / mean) if cores and mean > 0 else None,
    )


def report(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Result:
    clause, given = bounds("time", scope)
    # A minute the sampler credits with more CPU than the machine has is counted as all of
    # its cores busy, and counted in `over_cores`.
    minutes, seconds, used, peak, cores, saturated, over = db.execute(
        f"""SELECT count(*), coalesce(sum(seconds), 0),
            coalesce(sum(min(cpu_seconds, cores * seconds)), 0),
            max(min(cpu_seconds / seconds, cores)), max(cores),
            coalesce(sum(cpu_seconds >= ? * cores * seconds), 0),
            coalesce(sum(cpu_seconds > cores * seconds), 0)
        FROM cpu_minutes WHERE 1{clause}""",
        (SATURATED, *given),
    ).fetchone()
    seconds, used = float(seconds), float(used)
    waits, narrow = queued(db, scope)
    claimed, shared = shared_cpu(db, facts, scope)
    shared_total = sum(shared.values())
    closed = len(facts.closed(scope))
    ingest_cpu = float(
        db.execute(
            "SELECT coalesce(sum(cpu_seconds), 0) FROM ingests WHERE 1"
            + bounds("started", scope)[0],
            bounds("started", scope)[1],
        ).fetchone()[0]
    )
    machine_seconds = seconds * float(cores or 0)
    sampler = cpu.sampler_cpu()
    sampler_cores = sampler[0] / sampler[1] if sampler and sampler[1] > 0 else None
    result = Machine(
        scope.range.since,
        scope.range.until,
        scope.range.name,
        scope.describe(),
        int(minutes),
        int(cores) if cores else None,
        round(used / seconds, 1) if seconds else None,
        round(float(peak), 1) if peak is not None else None,
        round(100 * int(saturated) / int(minutes), 1) if minutes else None,
        int(over),
        hours(sum(w.waited_hours for w in waits) * 3600),
        round(sum(w.core_hours for w in waits), 2),
        waits,
        narrow,
        hours(used),
        hours(claimed),
        hours(shared_total),
        round(shared_total / 3600 / (seconds / 86400), 2) if seconds else None,
        round(shared_total / 3600 / closed, 2) if closed else None,
        bar({k: v / 3600 for k, v in shared.items()}, shared_total / 3600, 2),
        Own(
            round(sampler_cores, 3) if sampler_cores is not None else None,
            (
                round(100 * sampler_cores / cores, 2)
                if sampler_cores is not None and cores
                else None
            ),
            round(ingest_cpu, 1),
            round(100 * ingest_cpu / machine_seconds, 3) if machine_seconds else None,
        ),
        capacity(facts, scope, int(cores) if cores else None),
    )
    flags = scope.flags()
    return Result(
        "machine",
        asdict(result),
        lines(result),
        [
            "efficiency metrics machine.cores-in-use " + flags,
            "efficiency metrics machine.saturation " + flags,
            "efficiency metrics machine.queued-core-hours-per-day " + flags,
            "efficiency sql 'select * from leases order by time desc limit 20'",
        ],
        describe(
            "since",
            "cores",
            "cores_in_use",
            "peak_cores",
            "saturation",
            "over_cores",
            "queued_hours",
            "queued_core_hours",
            "queued",
            "narrow",
            "core_hours",
            "bead_core_hours",
            "shared_core_hours",
            "shared",
            "own",
            "capacity",
        ),
    )


def lines(m: Machine) -> list[str]:
    head = f"machine  since {m.since or 'the start'} ({m.range})"
    found = [head + (f"  {m.filters}" if m.filters else "")]
    found.append(
        f"cores {number(m.cores, 0)}   in use {number(m.cores_in_use, 1)} mean, "
        f"{number(m.peak_cores, 1)} in the busiest minute   saturation {number(m.saturation, 1)}% of "
        f"{m.minutes} minutes"
        + (
            f"   ({m.over_cores} minutes sampled over the cores, counted as all busy)"
            if m.over_cores
            else ""
        )
    )
    found.append(
        f"queued for cores  {m.queued_hours:.2f} h waiting, "
        f"{m.queued_core_hours:.2f} core h"
    )
    if m.queued:
        found += table(
            ["class", "command", "leases", "waited h", "core h"],
            [
                [q.klass, q.command, q.leases, q.waited_hours, q.core_hours]
                for q in m.queued
            ],
        )
    if m.narrow:
        found += ["", "fewer tokens than wanted"]
        found += table(
            ["class", "command", "leases", "narrower", "mean cores", "mean wanted"],
            [
                [n.klass, n.command, n.leases, n.narrower, n.mean_cores, n.mean_wanted]
                for n in m.narrow
            ],
        )
    found.append("")
    found.append(
        f"cpu       {m.core_hours:.2f} core h: beads {m.bead_core_hours:.2f} | shared "
        f"{m.shared_core_hours:.2f} ({number(m.shared_per_day, 2)} per day, "
        f"{number(m.shared_per_bead, 2)} per bead closed)"
    )
    found += split_lines("shared    ", m.shared, 2)
    own = m.own
    found.append(
        f"own cpu   sampler {number(own.sampler_cores, 3)} cores "
        f"({number(own.sampler_percent, 2)}%) | ingest {own.ingest_cpu_seconds:.1f} s "
        f"({number(own.ingest_percent, 3)}%)"
    )
    c = m.capacity
    found.append(
        f"capacity  cores per session {number(c.mean_cores, 1)} mean, "
        f"{number(c.p90_cores, 1)} p90 over {c.beads} beads; parallel capacity "
        f"{number(c.sessions, 0)} sessions"
    )
    return found


def bead_core_minutes(_: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    found: Points = []
    for b in facts.closed(scope):
        core = facts.core_minutes(b.id)
        began = b.began()
        if began and core is not None:
            found.append((began, core))
    return found


def daily(
    db: sqlite3.Connection, scope: Scope, query: str
) -> dict[str, tuple[str, float]]:
    """For each local day with sampled minutes in the range, its first minute and the sum of
    `query`'s second column over the rows its first column dates."""
    clause, given = bounds("time", scope)
    days: dict[str, tuple[str, float]] = {}
    for (moment,) in db.execute(
        f"SELECT time FROM cpu_minutes WHERE 1{clause} ORDER BY time", given
    ):
        days.setdefault(day_of(str(moment)), (str(moment), 0.0))
    for moment, value in db.execute(query + clause, given):
        day = day_of(str(moment))
        if day in days:
            first, sum_ = days[day]
            days[day] = (first, sum_ + float(value))
    return days


def saturation(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    clause, given = bounds("time", scope)
    counts: dict[str, list[int]] = {}
    firsts: dict[str, str] = {}
    for moment, full in db.execute(
        f"""SELECT time, cpu_seconds >= ? * cores * seconds FROM cpu_minutes
        WHERE 1{clause} ORDER BY time""",
        (SATURATED, *given),
    ):
        day = day_of(str(moment))
        firsts.setdefault(day, str(moment))
        counts.setdefault(day, []).append(int(full))
    return [
        (firsts[day], 100 * sum(found) / len(found)) for day, found in counts.items()
    ]


def cores_in_use(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    """Each day's mean cores in use: the sampled CPU seconds, each minute's at most all of
    the machine's cores, over the sampled seconds."""
    used = daily(
        db,
        scope,
        "SELECT time, min(cpu_seconds, cores * seconds) FROM cpu_minutes WHERE 1",
    )
    seconds = daily(db, scope, "SELECT time, seconds FROM cpu_minutes WHERE 1")
    return [
        (first, value / seconds[day][1])
        for day, (first, value) in used.items()
        if seconds[day][1] > 0
    ]


def queued_per_day(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    found = daily(
        db,
        scope,
        "SELECT time, waited_seconds * wanted / 3600.0 FROM leases WHERE 1",
    )
    return list(found.values())


register(
    Metric(
        "bead.core-minutes",
        "CPU minutes used in each closed bead's worktree",
        "minutes",
        1,
        bead_core_minutes,
    )
)
register(
    Metric(
        "machine.cores-in-use",
        "mean cores in use each day, from the CPU sampler's minutes",
        "cores",
        1,
        cores_in_use,
    )
)
register(
    Metric(
        "machine.saturation",
        "percent of each day's minutes with at least 90% of the cores in use",
        "percent",
        1,
        saturation,
    )
)
register(
    Metric(
        "machine.queued-core-hours-per-day",
        "core hours commands waited for CPU tokens each day: each wait times the "
        "tokens the command wanted",
        "core hours",
        2,
        queued_per_day,
    )
)
