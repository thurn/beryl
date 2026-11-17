"""The findings rules measured in agent hours (docs/efficiency-dashboard.md, "Findings"):
queued, enact-lock-wait, compile-share, repeated-command, waiting-on-person and
chained-commands."""

from __future__ import annotations

from collections import defaultdict

from . import kinds, locks
from .finding import (
    AGENT_HOURS,
    MIN_COHORT,
    Context,
    Item,
    Rule,
    bounds,
    cohort_of,
    grouped,
    median,
    percentile,
    top,
    total_cost,
    total_saving,
    wanted,
    window_role,
)
from .timing import seconds

HOUR = 3600.0


def hours(value: float | None) -> str:
    return "unknown" if value is None else f"{value:,.1f} h"


def window_category(c: Context, category: str, by_kind: bool) -> list[Item]:
    """The hours of the range's windows in `category`, by kind of command or by role."""
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    for w in c.facts.chosen(c.scope):
        found: dict[str, float] = defaultdict(float)
        for (name, kind, _), spent in w.time.items():
            if name == category:
                found[kind if by_kind else window_role(c.facts, w)] += spent / HOUR
        rows.extend((k, w.bead, v, 0.0, v) for k, v in found.items() if v > 0)
    return grouped(rows)


def queued(c: Context) -> list[Item]:
    """Time queued for cores, by kind of command."""
    return window_category(c, "queued", True)


def enact_lock_wait(c: Context) -> list[Item]:
    """Time sessions' Enact commands waited for Enact's ownership lock, by the command seen
    holding it. These are the waited seconds of the lock-wait log's lines joined to tool
    calls, so they can differ slightly from `enact lock wait` in `time --by kind`, which
    divides seconds among commands that ran at once."""
    return grouped(
        (holder, bead, waited / HOUR, 0.0, waited / HOUR)
        for holder, bead, waited in locks.held_up(c.db, c.facts, c.scope)
    )


def say_enact_lock_wait(items: list[Item]) -> str:
    return (
        f"Sessions waited {hours(total_cost(items))} a week for Enact's ownership lock, "
        f"most while {top(items)} held it"
    )


def waiting_on_person(c: Context) -> list[Item]:
    """Time waiting on a person, by role. It is outside agent minutes, but a person's
    reply is what holds the work."""
    return window_category(c, "person", False)


def compile_share(c: Context) -> list[Item]:
    """Compile time inside commands, by kind, against each kind's 10th percentile compile
    time over the range's commands of that kind that compiled. Only the kinds the command
    kinds table marks heavy count: a light command such as `cat` or `grep` over a build log
    prints cargo's `Finished ... in` lines without compiling anything."""
    clause, given = bounds("start", c.scope)
    keep = wanted(c.facts, c.scope)
    heavy = {k.kind for k in kinds.load()[0] if k.heavy}
    calls = [
        (str(kind), float(spent) / HOUR, bead)
        for kind, spent, bead in c.db.execute(
            f"""SELECT coalesce(kind, 'other'), compile_seconds, bead FROM tool_calls
            WHERE compile_seconds IS NOT NULL{clause}""",
            given,
        )
        if keep(bead) and kind in heavy
    ]
    by_kind: dict[str, list[float]] = defaultdict(list)
    for kind, spent, _ in calls:
        by_kind[kind].append(spent)
    low = {kind: percentile(found, 0.1) for kind, found in by_kind.items()}
    return grouped(
        (
            kind,
            bead,
            spent,
            min(spent, low[kind]),
            max(spent - low[kind], 0.0),
        )
        for kind, spent, bead in calls
    )


def repeated_command(c: Context) -> list[Item]:
    """For each cohort (role and tier) and kind of shell command that the cohort's median
    ticket runs, the tickets that ran it more often than the cohort's 90th percentile, against
    the cohort's median count at each ticket's own mean duration for that command."""
    closed = [b.id for b in c.facts.closed(c.scope)]
    cohorts: dict[str, list[str]] = defaultdict(list)
    for bead in closed:
        cohorts[cohort_of(c.facts, bead)].append(bead)
    chosen = set(closed)
    counts: dict[tuple[str, str], int] = defaultdict(int)
    spent: dict[tuple[str, str], float] = defaultdict(float)
    host = " AND host = ?" if c.scope.host else ""
    provider = " AND provider = ?" if c.scope.provider else ""
    for bead, kind, start, end in c.db.execute(
        f"""SELECT bead, coalesce(kind, 'other'), start, end FROM tool_calls
        WHERE command IS NOT NULL AND bead IS NOT NULL AND end IS NOT NULL{host}{provider}""",
        [v for v in (c.scope.host, c.scope.provider) if v],
    ):
        if bead in chosen:
            key = (str(bead), str(kind))
            counts[key] += 1
            spent[key] += max(seconds(str(end)) - seconds(str(start)), 0.0) / HOUR
    kinds = sorted({kind for _, kind in counts})
    rows: list[tuple[str, str | None, float, float, float | None]] = []
    for beads in cohorts.values():
        if len(beads) < MIN_COHORT:
            continue
        for kind in kinds:
            found = [float(counts.get((b, kind), 0)) for b in beads]
            high = percentile(found, 0.9)
            middle = median(found)
            # A kind most of the cohort never runs has no usual count to repeat: a ticket
            # that ran it at all would count its whole time.
            if middle < 1:
                continue
            for bead, count in zip(beads, found):
                if count > high and count > middle:
                    took = spent[(bead, kind)]
                    reference = middle * took / count
                    rows.append((kind, bead, took, reference, took - reference))
    return grouped(rows)


def chained_commands(c: Context) -> list[Item]:
    """Time in shell commands that chain two or more heavy commands, whose parts cannot be
    timed apart. A gap in measurement, not waste: its saving is unknown."""
    clause, given = bounds("start", c.scope)
    keep = wanted(c.facts, c.scope)
    return grouped(
        (str(kind), bead, took, took, None)
        for kind, start, end, bead in c.db.execute(
            f"""SELECT coalesce(kind, 'other'), start, end, bead FROM tool_calls
            WHERE chained = 1 AND end IS NOT NULL{clause}""",
            given,
        )
        if keep(bead)
        for took in [max(seconds(str(end)) - seconds(str(start)), 0.0) / HOUR]
    )


def say_queued(items: list[Item]) -> str:
    return (
        f"Commands waited {hours(total_cost(items))} a week for CPU tokens, most in "
        f"{top(items)}"
    )


def say_compile_share(items: list[Item]) -> str:
    return (
        f"Commands spent {hours(total_cost(items))} a week compiling; at each kind's 10th "
        f"percentile they would spend {hours(total_saving(items))} less, most in "
        f"{top(items)}"
    )


def say_repeated_command(items: list[Item]) -> str:
    count = len({b for i in items for b in i.beads})
    return (
        f"{count} tickets ran a command more often than 90% of their cohort; at the "
        f"cohort's median count they would save {hours(total_saving(items))} a week, most "
        f"in {top(items)}"
    )


def say_waiting_on_person(items: list[Item]) -> str:
    return (
        f"Sessions waited {hours(total_cost(items))} a week on a person, most in "
        f"{top(items)} sessions"
    )


def say_chained_commands(items: list[Item]) -> str:
    count = sum(i.count for i in items)
    return (
        f"{count} chained commands took {hours(total_cost(items))} a week that cannot be "
        f"split by kind, most first named {top(items)}"
    )


RULES: tuple[Rule, ...] = (
    Rule(
        "queued",
        AGENT_HOURS,
        "zero: no command waits for CPU tokens",
        queued,
        say_queued,
        "efficiency machine",
    ),
    Rule(
        "enact-lock-wait",
        AGENT_HOURS,
        "zero: no Enact command waits for the ownership lock",
        enact_lock_wait,
        say_enact_lock_wait,
        "efficiency enact",
    ),
    Rule(
        "compile-share",
        AGENT_HOURS,
        "for heavy commands, the same commands at the 10th percentile compile time of the "
        "range's commands of the same kind that compiled",
        compile_share,
        say_compile_share,
        "efficiency time --by kind",
    ),
    Rule(
        "repeated-command",
        AGENT_HOURS,
        "for tickets that ran a kind of shell command more often than the 90th percentile "
        f"of their cohort (role and tier, at least {MIN_COHORT} closed tickets), the "
        "cohort's median count at the ticket's own mean duration for it; only kinds the "
        "cohort's median ticket runs at least once",
        repeated_command,
        say_repeated_command,
        "efficiency time --by kind",
    ),
    Rule(
        "waiting-on-person",
        AGENT_HOURS,
        "zero: no session waits on a person (this time is beside agent minutes, not in "
        "them)",
        waiting_on_person,
        say_waiting_on_person,
        "efficiency summary",
    ),
    Rule(
        "chained-commands",
        AGENT_HOURS,
        "unknown: chained commands are a gap in measurement, not waste",
        chained_commands,
        say_chained_commands,
        "efficiency time --by kind",
    ),
)
