"""What each slow-CI check is worth: the real bugs it caught per CPU-second it costs.

`by ci-value` reads slow CI's records in the run log (runlog.py, script `ci`) and the verdicts on
the checks' beads (verdicts.py), and reports for every check slow CI ran in the window: its runs,
its catches (failures judged `catch`: a fix to product code made it pass), the median CPU seconds
of one run of it, and its value, catches per CPU-second. Lowest value first, so the top of the
list is what slow CI should drop first.

CPU time, not wall time, is the cost: the machine is never quiet, so wall seconds measure the
load as much as the check. A record has `cpu_seconds` only since the run log started measuring
it. For a check with no measured record yet, the cost is estimated from its wall seconds times
its lane's measured CPU-to-wall ratio, and marked `estimated`.

Checks that always fail together form a group, such as the yours/theirs and phone/desktop
variants of one verb: a group costs the sum of its members but catches only once, so it is
scored, and cut, as one. Two checks are grouped when they failed in exactly the same slow-CI runs,
at least [`GROUP_FAILURES`] times. A run in which more than [`MASS`] checks of one lane failed
was broken as a whole (a build error, a crashed server), and its failures group nothing.

Slow CI runs only the lanes, and of the rust lane only the packages, a change can affect, so
most runs leave checks out, and a run's own total says little about what slow CI costs. The
cost of a full run ([`full_run`]) is instead the sum of each check's median CPU seconds, over
the checks slow CI still runs. [`cuts`] gives the budget detector (budget.py) the lowest-value
groups whose removal brings a full run under a budget.
"""

from __future__ import annotations

import argparse
import json
import statistics
from collections.abc import Iterable
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone

from . import runlog, verdicts

DAYS = 30
# A full run counts the checks that ran in one of their lane's last this many runs (`alive`).
RUNS = 10
GROUP_FAILURES = 2
MASS = 20

# A slow-CI run, as the run log can tell one apart: the host and the commit it tested.
RunKey = tuple[str, str]


@dataclass
class Numbers:
    """One check's records in the window."""

    lane: str
    runs: int = 0
    measured: list[float] = field(default_factory=list)
    # Each record's CPU seconds, measured or estimated (`cost`).
    costs: list[float] = field(default_factory=list)
    failures: set[RunKey] = field(default_factory=set)


@dataclass(frozen=True)
class Group:
    """Checks scored as one: their runs, catches and CPU seconds a run, summed."""

    checks: tuple[str, ...]
    runs: int
    catches: int
    cpu: float
    estimated: bool

    def value(self) -> float:
        """Catches per CPU-second; a check that costs nothing measurable and has caught a bug
        is worth the most."""
        if self.cpu > 0:
            return self.catches / self.cpu
        return float("inf") if self.catches else 0.0

    def name(self) -> str:
        more = len(self.checks) - 1
        return self.checks[0] + (f" (+{more} failing with it)" if more else "")

    def json(self) -> dict[str, object]:
        value = self.value()
        return {
            "checks": list(self.checks),
            "runs": self.runs,
            "catches": self.catches,
            "cpu_seconds": round(self.cpu, 3),
            "estimated": self.estimated,
            "value": None if value == float("inf") else round(value, 6),
        }


@dataclass
class Scores:
    """Every group, lowest value first, and for `full_run`: each run's CPU seconds by check, when
    each run started (its first record's time), each check's lane, and each check's cost, the
    median CPU seconds of one run of it (measured when any record was)."""

    groups: list[Group]
    runs: dict[RunKey, dict[str, float]]
    started: dict[RunKey, str] = field(default_factory=dict)
    lanes: dict[str, str] = field(default_factory=dict)
    cost: dict[str, float] = field(default_factory=dict)

    def since(self, after: str) -> Scores:
        """These scores with only the runs that started after `after` (a run-log time), and only
        the groups with a check in one of them."""
        runs = {k: v for k, v in self.runs.items() if self.started.get(k, "") > after}
        ran = {check for run in runs.values() for check in run}
        groups = [g for g in self.groups if ran.intersection(g.checks)]
        started = {k: self.started[k] for k in runs if k in self.started}
        return Scores(groups, runs, started, self.lanes, self.cost)


def slow_ci(records: Iterable[dict[str, object]]) -> list[dict[str, object]]:
    """Slow CI's own runs of checks: not a bisection's reruns, nor a backfilled record, nor a
    step's record standing in for checks logged beside it (`stand_ins`)."""
    ci = [
        r
        for r in records
        if r.get("script") == "ci"
        and not r.get("backfilled")
        and r.get("outcome") in ("pass", "fail")
    ]
    left_out = stand_ins(ci)
    return [r for i, r in enumerate(ci) if i not in left_out]


def stand_ins(ci: list[dict[str, object]]) -> set[int]:
    """The indexes of the records that stand for a whole step rather than a check. A step logs
    its own record (`lane:step`) when it reported no checks, or failed though none of the checks
    it reported did (runlog.Run.log): a build error, a crash. When the step reports checks of its
    own, that record never passes, and its cost is the whole step's, so it is no check to score
    or cut. Records from before runlog marked such a record `whole` are known by that: their
    check never passed in the window. A step that reports no checks, such as `python:pytest`,
    passes as its own record, and that record is the check."""
    step: dict[tuple[object, ...], int] = {}
    for r in ci:
        at = (r.get("host"), r.get("commit"), r.get("lane"), r.get("time"))
        step[at] = step.get(at, 0) + 1
    passed = {str(r.get("check")) for r in ci if r.get("outcome") == "pass"}
    return {
        i
        for i, r in enumerate(ci)
        if str(r.get("check")) not in passed
        or (
            r.get("whole")
            and step[(r.get("host"), r.get("commit"), r.get("lane"), r.get("time"))] > 1
        )
    }


def number(value: object) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def ratios(records: list[dict[str, object]]) -> tuple[dict[str, float], float]:
    """Each lane's measured CPU seconds per wall second, and the ratio over every lane."""
    cpu: dict[str, float] = {}
    wall: dict[str, float] = {}
    for r in records:
        measured, seconds = number(r.get("cpu_seconds")), number(r.get("seconds"))
        if measured is None or not seconds:
            continue
        lane = str(r.get("lane"))
        cpu[lane] = cpu.get(lane, 0.0) + measured
        wall[lane] = wall.get(lane, 0.0) + seconds
    total = sum(wall.values(), 0.0)
    overall = sum(cpu.values(), 0.0) / total if total else 1.0
    return {lane: cpu[lane] / wall[lane] for lane in cpu if wall[lane]}, overall


def cost(r: dict[str, object], lanes: dict[str, float], overall: float) -> float:
    """A record's CPU seconds: measured, or estimated from its wall seconds."""
    measured = number(r.get("cpu_seconds"))
    if measured is not None:
        return measured
    return (number(r.get("seconds")) or 0.0) * lanes.get(str(r.get("lane")), overall)


def key(r: dict[str, object]) -> RunKey:
    return str(r.get("host")), str(r.get("commit"))


def score(
    records: Iterable[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    since: datetime,
) -> Scores:
    """The groups of checks slow CI ran since `since`, lowest value first."""
    ci = slow_ci(records)
    lanes, overall = ratios(ci)
    found: dict[str, Numbers] = {}
    runs: dict[RunKey, dict[str, float]] = {}
    started: dict[RunKey, str] = {}
    failed: dict[tuple[RunKey, str], int] = {}
    for r in ci:
        check = str(r.get("check"))
        n = found.setdefault(check, Numbers(str(r.get("lane"))))
        n.runs += 1
        measured = number(r.get("cpu_seconds"))
        if measured is not None:
            n.measured.append(measured)
        spent = cost(r, lanes, overall)
        n.costs.append(spent)
        run = runs.setdefault(key(r), {})
        run[check] = run.get(check, 0.0) + spent
        when = str(r.get("time"))
        started[key(r)] = min(started.get(key(r), when), when)
        if r.get("outcome") == "fail":
            n.failures.add(key(r))
            failed[(key(r), n.lane)] = failed.get((key(r), n.lane), 0) + 1
    broken = {k for k, count in failed.items() if count > MASS}
    groups: dict[frozenset[RunKey], list[str]] = {}
    for check, n in sorted(found.items()):
        shared = frozenset(k for k in n.failures if (k, n.lane) not in broken)
        alone = frozenset({("", check)})
        groups.setdefault(
            shared if len(shared) >= GROUP_FAILURES else alone, []
        ).append(check)
    costs = {
        check: statistics.median(n.measured or n.costs) for check, n in found.items()
    }
    scored = [
        group(members, found, costs, history, since) for members in groups.values()
    ]
    lanes = {check: n.lane for check, n in found.items()}
    return Scores(sorted(scored, key=order), runs, started, lanes, costs)


def group(
    members: list[str],
    found: dict[str, Numbers],
    costs: dict[str, float],
    history: dict[str, list[verdicts.Entry]],
    since: datetime,
) -> Group:
    """Scores checks that fail together as one. A bug they caught together is one catch, told
    apart by the commit that failed."""
    caught: set[str] = set()
    for check in members:
        for e in history.get(check, []):
            try:
                recent = datetime.fromisoformat(str(e.get("time"))) >= since
            except ValueError:
                continue
            if recent and e.get("verdict") == "catch":
                caught.add(str(e.get("commit")))
    cpu = sum(costs[c] for c in members)
    estimated = any(not found[c].measured for c in members)
    runs = max(found[c].runs for c in members)
    return Group(tuple(members), runs, len(caught), cpu, estimated)


def order(g: Group) -> tuple[float, float, str]:
    """Lowest value first; of equal value, the costliest first, since cutting it saves most."""
    return g.value(), -g.cpu, g.checks[0]


def alive(scores: Scores, last: int | None = None) -> set[str]:
    """The checks that ran in one of their lane's `last` runs (of all, when not given): those
    slow CI still runs. A check deleted since is in older runs only."""
    by_lane: dict[str, list[tuple[str, RunKey]]] = {}
    for k, run in scores.runs.items():
        for lane in {scores.lanes.get(check, "") for check in run}:
            by_lane.setdefault(lane, []).append((scores.started.get(k, ""), k))
    found: set[str] = set()
    for lane, runs in by_lane.items():
        for _, k in sorted(runs)[-last if last else 0 :]:
            found.update(c for c in scores.runs[k] if scores.lanes.get(c, "") == lane)
    return found


def full_run(scores: Scores, cut: set[str], last: int | None = None) -> float:
    """The CPU seconds of a slow-CI run of every lane, without the checks in `cut`: the sum of
    each check's cost (`Scores.cost`), over the checks still run (`alive`)."""
    return sum((scores.cost.get(c, 0.0) for c in alive(scores, last) - cut), 0.0)


def cuts(scores: Scores, budget: float, last: int | None = None) -> list[Group]:
    """The lowest-value groups whose removal brings a full slow-CI run (`full_run`, of the
    checks in each lane's `last` runs when given) to `budget` CPU seconds or under; none when
    it is under already. A group that has caught a bug may be among them: whoever acts on the
    list moves such a group to the nightly instead of deleting it."""
    running = alive(scores, last)
    whole = sum((scores.cost.get(c, 0.0) for c in running), 0.0)
    chosen: list[Group] = []
    for g in scores.groups:
        if whole <= budget:
            break
        members = running.intersection(g.checks)
        if not members:
            continue
        chosen.append(g)
        whole -= sum(scores.cost.get(c, 0.0) for c in members)
    return chosen


# `by ci-value`.


def report(args: argparse.Namespace) -> int:
    since = datetime.now(timezone.utc) - timedelta(days=args.days)
    records = runlog.read_log(since)
    store = verdicts.STORE
    scores = score(records, store.history(store.checks()), since)
    chosen = cuts(scores, args.budget, RUNS) if args.budget is not None else []
    whole = full_run(scores, set(), RUNS)
    if args.json:
        print(
            json.dumps(
                {
                    "since": since.isoformat(timespec="seconds"),
                    "runs": len(scores.runs),
                    "full_run_cpu_seconds": round(whole, 1),
                    "groups": [g.json() for g in scores.groups],
                    "cuts": [g.json() for g in chosen],
                },
                indent=2,
            )
        )
        return 0
    print(
        f"ci-value: {len(scores.groups)} checks and groups over {len(scores.runs)} slow-CI runs"
        f" in the last {args.days} days; a full run uses {whole:.0f} CPU seconds (each"
        f" check's median, summed over the checks of each lane's last {RUNS} runs)"
    )
    print(f"\n{'value':>9} {'catches':>7} {'runs':>5} {'cpu s':>8}  check")
    for g in scores.groups:
        value = g.value()
        shown = "inf" if value == float("inf") else f"{value:.4f}"
        mark = "~" if g.estimated else " "
        print(f"{shown:>9} {g.catches:7} {g.runs:5} {g.cpu:7.2f}{mark}  {g.name()}")
    print("\n~ estimated from wall seconds: no CPU time measured yet")
    if args.budget is not None:
        after = full_run(scores, {c for g in chosen for c in g.checks}, RUNS)
        print(
            f"\nci-value: cutting {len(chosen)} checks and groups brings a full run"
            f" from {whole:.0f} to {after:.0f} CPU seconds (budget {args.budget:.0f})"
        )
        for g in chosen:
            print(f"cut: {g.name()}")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "ci-value",
        help="each slow-CI check's real catches per CPU-second, lowest first",
    )
    p.add_argument("--days", type=int, default=DAYS, help=f"how far back ({DAYS})")
    p.add_argument("--json", action="store_true", help="JSON instead of a table")
    p.add_argument(
        "--budget",
        type=float,
        help="also list the cuts that bring a full run under this many CPU seconds",
    )
    p.set_defaults(func=report)
