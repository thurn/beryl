"""`just efficiency failures`: what each slow-CI failure costs beyond the failing check's own
run (docs/efficiency-dashboard.md, "What a slow-CI failure costs").

When slow CI fails on `staging`, Enact files a Steward ticket naming the commit range that
broke it (`enact_ci_range`, `A..B`), and `master` stops moving forward until a later run
passes. A failure's costs are:

- The Steward ticket: the agent minutes, core minutes and dollars of the sessions that worked
  it, from all its windows.
- Bisection: the CPU of the runs of the script `bisect` on commits in the range, which look
  for the commit at fault, estimated from wall seconds where unmeasured as the `ci` query
  estimates CPU. Ranges can overlap, so each run counts for the earliest Steward ticket whose
  range holds its commit. Where the store has no commits for the range, the bisection runs
  between the failure and the next passing run are counted instead.
- Time behind: from the end of the first failing slow-CI run on the range's last commit to
  the end of the next slow-CI run that passed, on any host the run log covers. It is unknown
  when the failing run is not in this machine's run log (the other CI machine failed) and
  while no run has passed since.
- The failed checks, from the verdicts whose `steward` is the ticket, with each `miss` (a check
  the fast check did not choose to run for the commit at fault) listed apart.

Units are never converted into one another: the Steward's core minutes and bisection's are
added into the failure's core minutes, and dollars and minutes behind stay figures of their
own. A failure is in the range when it happened in it: its failing run's end, or the Steward
ticket's creation when the run log has no failing run.

Its metrics are `slow-ci.failure-cost-per-day` and `staging.minutes-to-master`.
"""

from __future__ import annotations

import sqlite3
from collections import defaultdict
from dataclasses import asdict, dataclass, field

from . import ticket_ids
from .ci import ratios
from .cilog import iso, utc
from .facts import Facts, minutes, money, total
from .metrics import Metric, Points, register
from .render import Result, describe, table
from .rollups import day_of, number
from .scope import Scope

MISS = "miss"


@dataclass(frozen=True)
class CiRun:
    """One slow-CI run: every lane run of one commit on one host."""

    host: str
    commit: str
    end: str
    failed: bool
    steward: str | None = None


@dataclass(frozen=True)
class BisectRun:
    commit: str
    start: str
    cpu_seconds: float


@dataclass
class FailedCheck:
    check: str
    lane: str
    verdict: str


@dataclass
class StewardCost:
    """The Steward ticket's sessions' cost; dollars and agent minutes are None when the ticket
    ran on a host the store has no sessions from."""

    dollars: float | None
    agent_minutes: float | None
    core_minutes: float | None


@dataclass
class Failure:
    steward: str
    title: str | None
    status: str | None
    range: str
    failed_at: str
    host: str | None
    no_data: bool
    dollars: float | None
    agent_minutes: float | None
    steward_core_minutes: float | None
    bisect_core_minutes: float
    core_minutes: float
    behind_minutes: float | None
    checks: list[FailedCheck] = field(default_factory=list)


@dataclass
class Day:
    day: str
    failures: int
    dollars: float
    core_minutes: float
    behind_minutes: float


@dataclass
class Miss:
    check: str
    lane: str
    steward: str


@dataclass
class Failures:
    since: str | None
    until: str | None
    range: str
    failures: int
    no_data: int
    dollars: float
    agent_minutes: float
    core_minutes: float
    bisect_core_minutes: float
    behind_minutes: float
    unknown_behind: int
    days: list[Day]
    rows: list[Failure]
    misses: list[Miss]


def same_commit(a: str, b: str) -> bool:
    """Whether two commit names, either of which may be abbreviated, name one commit."""
    return bool(a) and bool(b) and (a.startswith(b) or b.startswith(a))


def cost_of(
    spec: str,
    created: str,
    runs: list[CiRun],
    bisect: list[BisectRun],
    in_range: set[str],
    steward: str | None = None,
) -> tuple[str, str | None, float, float | None]:
    """When one failure happened and on which host, its bisection's CPU seconds, and the
    minutes `master` stayed behind; `runs` are slow CI's runs in time order."""
    last = spec.partition("..")[2]
    failures = [r for r in runs if r.failed and same_commit(r.commit, last)]
    failing = next((r for r in failures if r.steward == steward), None)
    if failing is None:
        failing = next((r for r in failures if r.steward is None), None)
    passed = (
        next((r for r in runs if not r.failed and r.end > failing.end), None)
        if failing
        else None
    )
    failed_at = failing.end if failing else created
    if in_range:
        chosen = [b for b in bisect if any(same_commit(b.commit, c) for c in in_range)]
    else:
        until = passed.end if passed else None
        chosen = [
            b
            for b in bisect
            if b.start >= failed_at and (until is None or b.start < until)
        ]
    behind = (
        (utc(passed.end) - utc(failing.end)).total_seconds() / 60
        if failing and passed
        else None
    )
    return (
        failed_at,
        failing.host if failing else None,
        sum(b.cpu_seconds for b in chosen),
        behind,
    )


def failure(
    steward: str,
    title: str | None,
    status: str | None,
    spec: str,
    created: str,
    runs: list[CiRun],
    bisect: list[BisectRun],
    in_range: set[str],
    checks: list[FailedCheck],
    paid: StewardCost,
) -> Failure:
    """One failure's costs: the Steward ticket's, bisection's CPU and the time behind."""
    failed_at, host, bisect_cpu, behind = cost_of(
        spec, created, runs, bisect, in_range, steward
    )
    bisect_minutes = bisect_cpu / 60
    return Failure(
        steward,
        title,
        status,
        spec,
        failed_at,
        host,
        paid.dollars is None,
        None if paid.dollars is None else money(paid.dollars),
        None if paid.agent_minutes is None else round(paid.agent_minutes, 1),
        None if paid.core_minutes is None else round(paid.core_minutes, 1),
        round(bisect_minutes, 1),
        round((paid.core_minutes or 0.0) + bisect_minutes, 1),
        None if behind is None else round(behind, 1),
        checks,
    )


def ci_runs(db: sqlite3.Connection) -> list[CiRun]:
    """Slow CI's runs, one per host and commit, in the order they ended."""
    runs = {
        (str(host), str(commit), int(fails) > 0): CiRun(
            str(host), str(commit), str(end), int(fails) > 0
        )
        for host, commit, end, fails in db.execute(
            """SELECT host, "commit", max(end), sum(fails) FROM lane_runs
            WHERE script = 'ci' GROUP BY host, "commit\""""
        )
    }
    for time, host, tip, passed, ticket in db.execute(
        "SELECT time, host, tip, passed, ticket FROM ticket_ci_events ORDER BY seq"
    ):
        key = (str(host), str(tip), not passed)
        logged = runs.get(key)
        steward = ticket_ids.stored(db, str(ticket)) if ticket is not None else None
        if logged is None or (steward is not None and logged.steward is None):
            runs[key] = CiRun(
                key[0], key[1], logged.end if logged else str(time), key[2], steward
            )
    return sorted(runs.values(), key=lambda r: r.end)


def bisect_runs(db: sqlite3.Connection) -> list[BisectRun]:
    rates = ratios(db, "bisect", Scope())
    found: dict[str, list[float | str]] = {}
    for commit, lane, start, seconds, cpu, measured in db.execute(
        """SELECT "commit", lane, start, seconds, cpu_seconds, measured_seconds
        FROM lane_runs WHERE script = 'bisect'"""
    ):
        spent = float(cpu) + rates.estimate(str(lane), float(seconds) - float(measured))
        first, sum_ = found.get(str(commit), [str(start), 0.0])
        found[str(commit)] = [min(str(first), str(start)), float(sum_) + spent]
    return [BisectRun(c, str(s), float(cpu)) for c, (s, cpu) in found.items()]


def failed_checks(db: sqlite3.Connection) -> dict[str, list[FailedCheck]]:
    """Each Steward ticket's failed checks, once each however many runs and hosts recorded
    the failure."""
    found: dict[str, list[FailedCheck]] = defaultdict(list)
    for steward, check, lane, verdict in db.execute(
        """SELECT steward, "check", lane, verdict FROM verdicts
        WHERE steward IS NOT NULL GROUP BY steward, "check", lane, verdict
        ORDER BY min(time), "check" """
    ):
        found[str(steward)].append(FailedCheck(str(check), str(lane), str(verdict)))
    return found


def all_failures(db: sqlite3.Connection, facts: Facts) -> list[Failure]:
    """Every Steward ticket's failure, oldest first."""
    runs = ci_runs(db)
    bisect = bisect_runs(db)
    checks = failed_checks(db)
    history = facts.history()
    hosts = {w.host for w in facts.windows}
    found: list[Failure] = []
    # Steward ranges can overlap, so each bisection run is counted once, for the earliest
    # Steward ticket whose range holds its commit.
    claimed: set[str] = set()
    stewards = sorted(
        (
            b
            for b in facts.beads.values()
            if b.created and b.enact.get("enact_ci_range")
        ),
        key=lambda b: str(b.created),
    )
    for b in stewards:
        spec = b.enact.get("enact_ci_range")
        if not spec or not b.created:
            continue
        mine = [w for w in facts.windows if w.kind == "bead" and w.bead == b.id]
        sums = total(mine)
        ran = b.ran_on()
        unseen = not mine and ran is not None and ran not in hosts
        paid = StewardCost(
            None if unseen else sums.dollars,
            None if unseen else minutes(sums.agent_seconds),
            facts.core_minutes(b.id),
        )
        in_range = {c.sha for c in history.range(str(spec))}
        unclaimed = [x for x in bisect if x.commit not in claimed]
        claimed |= {
            x.commit
            for x in unclaimed
            if any(same_commit(x.commit, c) for c in in_range)
        }
        found.append(
            failure(
                b.id,
                b.title,
                b.status,
                str(spec),
                iso(utc(b.created)),
                runs,
                unclaimed,
                in_range,
                checks.get(b.id, []),
                paid,
            )
        )
    return sorted(found, key=lambda f: f.failed_at)


def chosen(db: sqlite3.Connection, facts: Facts, scope: Scope) -> list[Failure]:
    """The failures that happened in the range, of Steward tickets the filters choose."""
    cohort = facts.cohort(scope)
    since, until = scope.range.since, scope.range.until
    return [
        f
        for f in all_failures(db, facts)
        if (since is None or f.failed_at >= since)
        and (until is None or f.failed_at < until)
        and (cohort is None or f.steward in cohort)
        and (scope.host is None or f.host in (None, scope.host))
    ]


def report(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Result:
    found = chosen(db, facts, scope)
    days: dict[str, Day] = {}
    for f in found:
        day = days.setdefault(day_of(f.failed_at), Day(day_of(f.failed_at), 0, 0, 0, 0))
        day.failures += 1
        day.dollars += f.dollars or 0.0
        day.core_minutes += f.core_minutes
        day.behind_minutes += f.behind_minutes or 0.0
    result = Failures(
        scope.range.since,
        scope.range.until,
        scope.range.name,
        len(found),
        sum(1 for f in found if f.no_data),
        money(sum(f.dollars or 0.0 for f in found)),
        round(sum(f.agent_minutes or 0.0 for f in found), 1),
        round(sum(f.core_minutes for f in found), 1),
        round(sum(f.bisect_core_minutes for f in found), 1),
        round(sum(f.behind_minutes or 0.0 for f in found), 1),
        sum(1 for f in found if f.behind_minutes is None),
        [
            Day(
                d.day,
                d.failures,
                money(d.dollars),
                round(d.core_minutes, 1),
                round(d.behind_minutes, 1),
            )
            for d in sorted(days.values(), key=lambda d: d.day)
        ],
        list(reversed(found)),
        [
            Miss(c.check, c.lane, f.steward)
            for f in found
            for c in f.checks
            if c.verdict == MISS
        ],
    )
    flags = scope.flags()
    more = [f"efficiency metrics slow-ci.failure-cost-per-day {flags}"]
    if result.rows:
        more.insert(0, f"efficiency ticket {result.rows[0].steward}")
    return Result(
        "failures",
        asdict(result),
        lines(result),
        [m.strip() for m in more],
        {**describe("since"), **FIELDS},
    )


# How many failed checks a failure's row names before it counts the rest.
NAMED = 2


def checks_text(checks: list[FailedCheck]) -> str:
    named = ", ".join(f"{c.check} ({c.verdict})" for c in checks[:NAMED])
    rest = len(checks) - NAMED
    return (named + (f" and {rest} more" if rest > 0 else "")) or "-"


def lines(f: Failures) -> list[str]:
    found = [
        f"what slow-CI failures cost  since {f.since or 'the start'} ({f.range})",
        f"failures {f.failures}"
        + (f" ({f.no_data} with stewards on hosts with no data)" if f.no_data else "")
        + f"   steward dollars {f.dollars:.2f}   agent min "
        f"{f.agent_minutes:.1f}   core min {f.core_minutes:.1f} (bisection "
        f"{f.bisect_core_minutes:.1f})",
        f"master behind staging {f.behind_minutes:.0f} min"
        + (f", unknown for {f.unknown_behind}" if f.unknown_behind else "")
        + f"   misses {len(f.misses)}",
        "",
    ]
    found += table(
        ["day", "failures", "dollars", "core min", "behind min"],
        [
            [d.day, d.failures, d.dollars, d.core_minutes, d.behind_minutes]
            for d in f.days
        ],
    )
    found += ["", "failures, newest first"]
    found += table(
        [
            "failed at",
            "steward",
            "dollars",
            "agent min",
            "core min",
            "bisect min",
            "behind min",
            "checks",
        ],
        [
            [
                x.failed_at[:16],
                x.steward,
                number(x.dollars, 2),
                number(x.agent_minutes, 1),
                x.core_minutes,
                x.bisect_core_minutes,
                number(x.behind_minutes, 1),
                checks_text(x.checks),
            ]
            for x in f.rows
        ],
    )
    if f.misses:
        found += [
            "",
            "misses: checks the fast check did not choose for the commit at fault",
        ]
        found += [f"  {m.check}  ({m.lane}, {m.steward})" for m in f.misses]
    return found


FIELDS: dict[str, str] = {
    "failures": "slow-CI failures in the range: one per Steward ticket, by when the failing "
    "run ended, or when the ticket was made if the run log has no failing run",
    "steward": "the Steward ticket filed to fix the failure",
    "failed_at": "the end of the first failing slow-CI run on the range's last commit, or "
    "the Steward ticket's creation",
    "no_data": "failures whose Steward ticket ran on a host the store has no sessions "
    "from, whose dollars and agent minutes are unknown",
    "dollars": "dollars of the Steward ticket's sessions; null when it ran on a host with no "
    "data, and the totals count the known ones",
    "agent_minutes": "agent minutes of the Steward ticket's sessions; null when it ran on a "
    "host with no data",
    "steward_core_minutes": "CPU minutes in the Steward ticket's worktree while it was open; "
    "null where the sampler was not running",
    "bisect_core_minutes": "CPU minutes of bisection runs on commits in the range",
    "core_minutes": "the Steward ticket's CPU minutes and bisection's, added",
    "behind_minutes": "minutes from the failing run's end to the next passing slow-CI "
    "run's end, while master could not move forward; null when either is not in this "
    "machine's run log",
    "unknown_behind": "failures whose minutes behind are unknown",
    "checks": "the failed checks the Steward fixed, with their verdicts",
    "days": "failures and their costs by the local day they happened",
    "misses": "failed checks the fast check had not chosen for the commit at fault",
}


def cost_per_day(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    days: dict[str, list[float | str]] = {}
    for f in chosen(db, facts, scope):
        first, dollars = days.get(day_of(f.failed_at), [f.failed_at, 0.0])
        days[day_of(f.failed_at)] = [
            min(str(first), f.failed_at),
            float(dollars) + (f.dollars or 0.0),
        ]
    return sorted((str(first), float(d)) for first, d in days.values())


def to_master(_: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    """Minutes from each commit in the range to its reaching `master`."""
    history = facts.history()
    since, until = scope.range.since, scope.range.until
    found: Points = []
    for c in history.commits.values():
        reached = history.on_master.get(c.sha)
        when = iso(utc(c.time))
        if reached is None or (since and when < since) or (until and when >= until):
            continue
        found.append(
            (when, max((utc(reached) - utc(c.time)).total_seconds() / 60, 0.0))
        )
    return sorted(found)


register(
    Metric(
        "slow-ci.failure-cost-per-day",
        "dollars of the Steward tickets of each day's slow-CI failures",
        "dollars",
        2,
        cost_per_day,
    )
)
register(
    Metric(
        "staging.minutes-to-master",
        "minutes from each commit on staging (its commit time) to its reaching master",
        "minutes",
        0,
        to_master,
    )
)
