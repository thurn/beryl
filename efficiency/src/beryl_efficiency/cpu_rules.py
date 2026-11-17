"""The findings rules measured in core hours (docs/efficiency-dashboard.md, "Findings"):
narrow-leases, never-caught, flake-cost and misses."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import replace

from . import ci, failures
from .cilog import family
from .finding import (
    CORE_HOURS,
    Context,
    Item,
    Rule,
    bounds,
    grouped,
    top,
    total_saving,
)
from .scope import Range

HOUR = 3600.0
# A check family needs this many runs with no catch before its CPU counts as waste.
NEVER_CAUGHT_RUNS = 100
# The scripts whose checks the CI rules look at: every one but bisection, whose runs are
# counted with the failure they look for.
SCRIPTS: tuple[str, ...] = ("ci", "check", "sweep", "nightly")
# The verdicts that mean a check found a real bug.
CATCHES: tuple[str, ...] = ("catch", "mixed")


def core_hours(value: float | None) -> str:
    return "unknown" if value is None else f"{value:,.1f} core h"


def worktree_beads(c: Context) -> dict[str, str]:
    """The ticket each worktree name belongs to: the last one to start there."""
    found: dict[str, tuple[str, str]] = {}
    for b in c.facts.beads.values():
        name = c.facts.worktree(b)
        began = b.began() or ""
        if name and (name not in found or began > found[name][0]):
            found[name] = (began, b.id)
    return {name: bead for name, (_, bead) in found.items()}


def narrow_leases(c: Context) -> list[Item]:
    """For leases that got fewer CPU tokens than they wanted, the tokens missing times the
    seconds held, by class and command."""
    clause, given = bounds("time", c.scope, provider=False)
    owners = worktree_beads(c)
    return grouped(
        (
            f"{klass} {command}",
            owners.get(str(worktree)),
            missing,
            0.0,
            missing,
        )
        for klass, command, worktree, cores, wanted, held in c.db.execute(
            f"""SELECT class, command, worktree, cores, wanted, held_seconds FROM leases
            WHERE cores < wanted{clause}""",
            given,
        )
        for missing in [(int(wanted) - int(cores)) * float(held) / HOUR]
    )


def never_caught(c: Context) -> list[Item]:
    """The range's CPU of check families with no catch in all their runs, once they have
    run at least 100 times, by script and family."""
    ever = replace(c.scope, range=Range(None, None, "all"))
    items: list[Item] = []
    for script in SCRIPTS:
        always = ci.grouped(ci.checks(c.db, c.facts, ever, script), "family")
        recent = ci.grouped(ci.checks(c.db, c.facts, c.scope, script), "family")
        for name, raw in recent.items():
            whole = always.get(name)
            if whole is None or whole.runs < NEVER_CAUGHT_RUNS:
                continue
            if any(whole.verdicts.get(v, 0) for v in CATCHES) or raw.cpu <= 0:
                continue
            spent = raw.cpu / HOUR
            items.append(
                Item(f"{ci.SCRIPTS[script]}: {name}", raw.runs, spent, 0.0, spent)
            )
    return sorted(items, key=lambda i: (-(i.saving or 0.0), i.name))


def flake_cost(c: Context) -> list[Item]:
    """One rerun per flaky failure at the check's mean CPU per run, by script and check."""
    items: list[Item] = []
    for script in SCRIPTS:
        for check, raw in ci.checks(c.db, c.facts, c.scope, script).items():
            if raw.flake_cpu > 0:
                spent = raw.flake_cpu / HOUR
                items.append(
                    Item(
                        f"{ci.SCRIPTS[script]}: {check.name}",
                        raw.verdicts.get("flaky", 0),
                        spent,
                        0.0,
                        spent,
                    )
                )
    return sorted(items, key=lambda i: (-(i.saving or 0.0), i.name))


def failed_ci_cpu(c: Context) -> dict[str, float]:
    """The CPU seconds of each slow-CI run (all its lanes on one host and commit) that had
    a failure, by commit, with unmeasured seconds estimated as the `ci` query does."""
    rates = ci.ratios(c.db, "ci", replace(c.scope, range=Range(None, None, "all")))
    runs: dict[tuple[str, str], list[tuple[str, float, float, float]]] = defaultdict(
        list
    )
    for host, commit, lane, fails, wall, cpu, measured in c.db.execute(
        """SELECT host, "commit", lane, fails, seconds, cpu_seconds, measured_seconds
        FROM lane_runs WHERE script = 'ci'"""
    ):
        runs[(str(host), str(commit))].append(
            (str(lane), float(fails), float(cpu), float(wall) - float(measured))
        )
    found: dict[str, float] = defaultdict(float)
    for (_, commit), lanes in runs.items():
        if any(fails for _, fails, _, _ in lanes):
            found[commit] += sum(
                cpu + rates.estimate(lane, rest) for lane, _, cpu, rest in lanes
            )
    return found


def misses(c: Context) -> list[Item]:
    """For each slow-CI failure in the range with a miss (a failing check the fast check had
    not chosen), its bisection's CPU and the failed slow-CI runs on its commit range."""
    found = [
        f
        for f in failures.chosen(c.db, c.facts, c.scope)
        if any(x.verdict == failures.MISS for x in f.checks)
    ]
    if not found:
        return []
    failed = failed_ci_cpu(c)
    history = c.facts.history()
    items: list[Item] = []
    for f in found:
        shas = [x.sha for x in history.range(f.range)]
        runs = sum(
            cpu
            for commit, cpu in failed.items()
            if any(failures.same_commit(commit, s) for s in shas)
        )
        spent = f.bisect_core_minutes / 60 + runs / HOUR
        missed = sorted({x.check for x in f.checks if x.verdict == failures.MISS})
        items.append(
            Item(
                f"{f.steward}: {', '.join(missed)}",
                len(missed),
                spent,
                0.0,
                spent,
                {f.steward: spent},
            )
        )
    return sorted(items, key=lambda i: (-(i.saving or 0.0), i.name))


def say_narrow_leases(items: list[Item]) -> str:
    return (
        f"Leases that got fewer CPU tokens than they wanted went without "
        f"{core_hours(total_saving(items))} a week, most in {top(items)}"
    )


def say_never_caught(items: list[Item]) -> str:
    return (
        f"{len(items)} check families with {NEVER_CAUGHT_RUNS}+ runs and no catch ever "
        f"used {core_hours(total_saving(items))} a week, most in {top(items)}"
    )


def say_flake_cost(items: list[Item]) -> str:
    count = sum(i.count for i in items)
    return (
        f"Reruns after {count} flaky failures cost {core_hours(total_saving(items))} a "
        f"week, most for {top(items)}"
    )


def say_misses(items: list[Item]) -> str:
    return (
        f"{len(items)} slow-CI failures the fast check missed cost "
        f"{core_hours(total_saving(items))} a week in bisection and failed runs"
    )


RULES: tuple[Rule, ...] = (
    Rule(
        "narrow-leases",
        CORE_HOURS,
        "zero: every lease gets the CPU tokens it wants",
        narrow_leases,
        say_narrow_leases,
        "efficiency machine",
    ),
    Rule(
        "never-caught",
        CORE_HOURS,
        f"zero, for check families that have run {NEVER_CAUGHT_RUNS} times or more with "
        "no catch (or mixed verdict) in all their runs",
        never_caught,
        say_never_caught,
        "efficiency ci",
    ),
    Rule(
        "flake-cost",
        CORE_HOURS,
        "zero: no rerun after a flaky failure",
        flake_cost,
        say_flake_cost,
        "efficiency ci",
    ),
    Rule(
        "misses",
        CORE_HOURS,
        "zero: the fast check runs the check that would have failed",
        misses,
        say_misses,
        "efficiency failures",
    ),
)
