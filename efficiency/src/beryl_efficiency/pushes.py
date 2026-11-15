"""`just efficiency pushes`: what the fast check costs each push (docs/efficiency-dashboard.md,
"The fast check" and "Inside a push").

`enact push` rebases onto `staging`, runs the fast check and pushes. Its tool call is split by
the run log: the `lane_runs` rows of the fast check (script `check`) in the bead's worktree
that started and ended inside the call give the seconds of each check lane, and what remains
of the call is rebase, waiting and push. How the figures are made:

- A push is a tool call of kind `push`. One run in the background ends when the session is
  told it finished (a `prompts` row, or a `notifications` row for a notice that came while
  the session was busy), not when the call returned.
- The worktree is the directory name of the bead's `enact_worktree`. Run-log lines written
  before the run log recorded worktrees name none, so their pushes, and pushes with no bead,
  cannot be joined and are reported as not split.
- Check lanes run at once, so the fast check's part of a push is the time covered by at least
  one of its lane runs, not their sum; each lane's seconds divide that part and are not added
  again.
- The check runs again, on a new commit, when the push loses the race to `staging` and
  rebases. Each run is one commit and tree; the runs after the first are repeats, with their
  covered seconds and CPU.
- CPU is the run log's measured CPU, with unmeasured lines estimated from wall seconds as the
  `ci` query estimates them. Seconds queued for cores are the waits of the CPU leases joined
  to the call.
- Steps left to slow CI come from the push's output, which records the steps cut at the
  hard limit and the Rust tests cut to fit the time left; a push run in the background has
  no output in its transcript.
- Failures before a push are the verdicts the fast check recorded, by check lane.

Its metrics are `push.wall-seconds` and `push.repeats`.
"""

from __future__ import annotations

import json
import sqlite3
import statistics
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass, field
from datetime import timedelta

from .ci import Ratios, ratios
from .cilog import VERDICTS, iso, utc
from .facts import Facts
from .metrics import Metric, Points, register
from .render import Result, describe, table
from .rollups import number
from .scope import Scope

SCRIPT = "check"
# How many pushes the text lists, newest first.
PUSH_ROWS = 30
# Run-log times are a step's end less its seconds, so a lane run may appear to start a
# moment before the call that ran it.
SLACK = timedelta(seconds=2)


@dataclass(frozen=True)
class LaneRow:
    """One `lane_runs` row of the fast check."""

    commit: str
    tree: str
    lane: str
    start: float
    end: float
    fails: int
    seconds: float
    cpu_seconds: float
    measured_seconds: float


@dataclass
class PushLane:
    lane: str
    seconds: float
    cpu_seconds: float
    runs: int


@dataclass
class Push:
    bead: str | None
    host: str
    provider: str
    transcript: str
    call: str
    start: str
    seconds: float
    split: bool
    fast_check_seconds: float | None
    rest_seconds: float | None
    cpu_seconds: float | None
    queued_seconds: float
    checks: int
    repeats: int
    repeat_seconds: float
    repeat_cpu_seconds: float
    failed: bool | None
    lanes: list[PushLane] = field(default_factory=list)
    deferred: list[str] = field(default_factory=list)


@dataclass
class Count:
    name: str
    count: int


@dataclass
class LaneFailures:
    lane: str
    verdicts: dict[str, int]


@dataclass
class FastCheck:
    since: str | None
    until: str | None
    range: str
    pushes: int
    split: int
    median_wall_seconds: float | None
    median_fast_check_seconds: float | None
    median_rest_seconds: float | None
    cpu_seconds: float
    queued_seconds: float
    repeats: int
    repeat_seconds: float
    repeat_cpu_seconds: float
    lanes: list[PushLane]
    deferred: list[Count]
    failures: dict[str, int]
    failures_by_lane: list[LaneFailures]
    rows: list[Push]


def covered(spans: list[tuple[float, float]]) -> float:
    """The seconds covered by at least one of `spans`."""
    total = 0.0
    reach: float | None = None
    for start, end in sorted(spans):
        if reach is None or start > reach:
            total += end - start
            reach = end
        elif end > reach:
            total += end - reach
            reach = end
    return total


def split(
    start: float,
    end: float,
    runs: list[LaneRow],
    rates: Ratios,
) -> tuple[float | None, float | None, list[PushLane], int, float, float, bool | None]:
    """A push call from `start` to `end` split by the fast check's lane runs inside it: the
    covered seconds, the CPU, each lane, the check runs, the repeats' covered seconds and
    CPU, and whether the last run failed. The covered seconds are None when no run joined.
    """
    if not runs:
        return None, None, [], 0, 0.0, 0.0, None
    by_check: dict[tuple[str, str], list[LaneRow]] = defaultdict(list)
    for r in runs:
        by_check[(r.commit, r.tree)].append(r)
    checks = sorted(by_check.values(), key=lambda rs: min(r.start for r in rs))

    def clipped(rs: list[LaneRow]) -> list[tuple[float, float]]:
        return [(max(r.start, start), min(r.end, end)) for r in rs if r.end > r.start]

    def cpu(rs: list[LaneRow]) -> float:
        return sum(
            r.cpu_seconds + rates.estimate(r.lane, r.seconds - r.measured_seconds)
            for r in rs
        )

    lanes: dict[str, PushLane] = {}
    for r in runs:
        found = lanes.setdefault(r.lane, PushLane(r.lane, 0.0, 0.0, 0))
        found.seconds += covered(clipped([r]))
        found.cpu_seconds += cpu([r])
        found.runs += 1
    repeated = [r for rs in checks[1:] for r in rs]
    return (
        covered(clipped(runs)),
        cpu(runs),
        sorted(lanes.values(), key=lambda p: (-p.seconds, p.lane)),
        len(checks),
        covered(clipped(repeated)),
        cpu(repeated),
        any(r.fails for r in checks[-1]),
    )


def lane_rows(
    db: sqlite3.Connection, host: str, worktree: str, start: str, end: str
) -> list[LaneRow]:
    """The fast check's lane runs in `worktree` that started and ended between `start` and
    `end`."""
    since = iso(utc(start) - SLACK)
    until = iso(utc(end) + SLACK)
    return [
        LaneRow(
            str(c),
            str(t),
            str(lane),
            utc(s).timestamp(),
            utc(e).timestamp(),
            int(fails),
            float(seconds),
            float(spent),
            float(measured),
        )
        for c, t, lane, s, e, fails, seconds, spent, measured in db.execute(
            """SELECT "commit", tree, lane, start, end, fails, seconds, cpu_seconds,
                measured_seconds FROM lane_runs
            WHERE script = ? AND host = ? AND worktree = ? AND start >= ? AND end <= ?""",
            (SCRIPT, host, worktree, since, until),
        )
    ]


def pushes(
    db: sqlite3.Connection, facts: Facts, scope: Scope, bead: str | None = None
) -> list[Push]:
    """Every push that started in the range, or only `bead`'s, oldest first, split where
    it can be."""
    clause = ""
    given: list[object] = []
    if bead is not None:
        clause += " AND t.bead = ?"
        given.append(bead)
    if scope.range.since:
        clause += " AND t.start >= ?"
        given.append(scope.range.since)
    if scope.range.until:
        clause += " AND t.start < ?"
        given.append(scope.range.until)
    if scope.host:
        clause += " AND t.host = ?"
        given.append(scope.host)
    if scope.provider:
        clause += " AND t.provider = ?"
        given.append(scope.provider)
    cohort = facts.cohort(scope)
    rates = ratios(db, SCRIPT, scope)
    found: list[Push] = []
    for (
        host,
        provider,
        transcript,
        call,
        start,
        end,
        owner,
        deferred,
        told,
        queued,
    ) in db.execute(
        f"""SELECT t.host, t.provider, t.transcript, t.call, t.start, t.end, t.bead,
                t.deferred,
                (SELECT min(time) FROM (
                    SELECT p.time FROM prompts AS p WHERE p.host = t.host
                        AND p.provider = t.provider AND p.kind = 'background'
                        AND p.detail = t.call
                    UNION ALL SELECT n.time FROM notifications AS n
                        WHERE n.host = t.host AND n.provider = t.provider
                        AND n.call = t.call)),
                (SELECT coalesce(sum(l.waited_seconds), 0) FROM leases AS l
                    WHERE l.host = t.host AND l.provider = t.provider
                    AND l.transcript = t.transcript AND l.call = t.call)
            FROM tool_calls AS t WHERE t.kind = 'push' AND t.end IS NOT NULL{clause}
            ORDER BY t.start""",
        given,
    ):
        if cohort is not None and owner not in cohort:
            continue
        finish = max(str(end), str(told)) if told else str(end)
        known = facts.beads.get(str(owner)) if owner else None
        worktree = facts.worktree(known) if known else ""
        runs = (
            lane_rows(db, str(host), worktree, str(start), finish) if worktree else []
        )
        began, ended = utc(start).timestamp(), utc(finish).timestamp()
        part, cpu, lanes, checks, repeat, repeat_cpu, failed = split(
            began, ended, runs, rates
        )
        seconds = round(ended - began, 1)
        fast = None if part is None else round(part, 1)
        found.append(
            Push(
                str(owner) if owner else None,
                str(host),
                str(provider),
                str(transcript),
                str(call),
                str(start),
                seconds,
                part is not None,
                fast,
                None if fast is None else round(seconds - fast, 1),
                None if cpu is None else round(cpu, 1),
                round(float(queued), 1),
                checks,
                max(checks - 1, 0),
                round(repeat, 1),
                round(repeat_cpu, 1),
                failed,
                [
                    PushLane(
                        p.lane, round(p.seconds, 1), round(p.cpu_seconds, 1), p.runs
                    )
                    for p in lanes
                ],
                [str(s) for s in json.loads(str(deferred))] if deferred else [],
            )
        )
    return found


def median(values: list[float]) -> float | None:
    return round(statistics.median(values), 1) if values else None


def failures(
    db: sqlite3.Connection, scope: Scope
) -> tuple[dict[str, int], list[LaneFailures]]:
    """The fast check's failures in the range by verdict, in all and by check lane."""
    clause, given = "", []
    if scope.range.since:
        clause += " AND time >= ?"
        given.append(scope.range.since)
    if scope.range.until:
        clause += " AND time < ?"
        given.append(scope.range.until)
    if scope.host:
        clause += " AND host = ?"
        given.append(scope.host)
    by_lane: dict[str, dict[str, int]] = defaultdict(dict)
    total: dict[str, int] = {v: 0 for v in VERDICTS}
    for lane, verdict, count in db.execute(
        f"""SELECT lane, verdict, count(*) FROM verdicts WHERE script = ?{clause}
        GROUP BY lane, verdict""",
        (SCRIPT, *given),
    ):
        by_lane[str(lane)][str(verdict)] = int(count)
        total[str(verdict)] = total.get(str(verdict), 0) + int(count)
    return total, [
        LaneFailures(lane, verdicts)
        for lane, verdicts in sorted(
            by_lane.items(), key=lambda kv: -sum(kv[1].values())
        )
    ]


def report(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Result:
    found = pushes(db, facts, scope)
    joined = [p for p in found if p.split]
    lanes: dict[str, PushLane] = {}
    for p in joined:
        for lane in p.lanes:
            sum_ = lanes.setdefault(lane.lane, PushLane(lane.lane, 0.0, 0.0, 0))
            sum_.seconds += lane.seconds
            sum_.cpu_seconds += lane.cpu_seconds
            sum_.runs += lane.runs
    steps = Counter(step for p in found for step in p.deferred)
    total, by_lane = failures(db, scope)
    result = FastCheck(
        scope.range.since,
        scope.range.until,
        scope.range.name,
        len(found),
        len(joined),
        median([p.seconds for p in found]),
        median([p.fast_check_seconds or 0.0 for p in joined]),
        median([p.rest_seconds or 0.0 for p in joined]),
        round(sum(p.cpu_seconds or 0.0 for p in joined), 1),
        round(sum(p.queued_seconds for p in found), 1),
        sum(p.repeats for p in found),
        round(sum(p.repeat_seconds for p in found), 1),
        round(sum(p.repeat_cpu_seconds for p in found), 1),
        [
            PushLane(s.lane, round(s.seconds, 1), round(s.cpu_seconds, 1), s.runs)
            for s in sorted(lanes.values(), key=lambda s: (-s.cpu_seconds, s.lane))
        ],
        [Count(name, n) for name, n in steps.most_common()],
        total,
        by_lane,
        list(reversed(found))[:PUSH_ROWS],
    )
    flags = scope.flags()
    more = [
        f"efficiency ci --script check {flags}",
        f"efficiency metrics push.wall-seconds {flags}",
    ]
    if result.rows and result.rows[0].bead:
        more.insert(0, f"efficiency bead {result.rows[0].bead}")
    return Result(
        "pushes",
        asdict(result),
        lines(result),
        [m.strip() for m in more],
        {**describe("since"), **FIELDS},
    )


def push_line(p: Push) -> str:
    """One push as the `bead` query and the `pushes` query print it."""
    if not p.split:
        return f"push {p.start[:16]}  {p.seconds:.0f} s, not split (no run-log lines)"
    lanes = ", ".join(f"{lane.lane} {lane.seconds:.0f}" for lane in p.lanes)
    repeats = f", {p.repeats} repeats {p.repeat_seconds:.0f} s" if p.repeats else ""
    return (
        f"push {p.start[:16]}  {p.seconds:.0f} s: fast check {number(p.fast_check_seconds, 0)}"
        f" ({lanes}) | rebase and push {number(p.rest_seconds, 0)}{repeats}"
    )


def lines(f: FastCheck) -> list[str]:
    found = [
        f"the fast check per push  since {f.since or 'the start'} ({f.range})",
        f"pushes {f.pushes}   split by the run log {f.split}   median wall s "
        f"{number(f.median_wall_seconds, 1)}: fast check "
        f"{number(f.median_fast_check_seconds, 1)} | rebase and push "
        f"{number(f.median_rest_seconds, 1)}",
        f"cpu s {f.cpu_seconds:.0f}   queued for cores s {f.queued_seconds:.0f}   repeats "
        f"{f.repeats} ({f.repeat_seconds:.0f} s, {f.repeat_cpu_seconds:.0f} cpu s)",
        "failures before a push   "
        + " | ".join(f"{v} {f.failures.get(v, 0)}" for v in VERDICTS),
        "",
    ]
    found += table(
        ["check lane", "runs", "seconds", "cpu s"],
        [[lane.lane, lane.runs, lane.seconds, lane.cpu_seconds] for lane in f.lanes],
    )
    if f.deferred:
        found += ["", "left to slow CI"]
        found += [f"  {c.name}  {c.count}" for c in f.deferred]
    if f.failures_by_lane:
        found += ["", "failures before a push by check lane"]
        found += table(
            ["check lane", *VERDICTS],
            [
                [x.lane, *(x.verdicts.get(v, 0) for v in VERDICTS)]
                for x in f.failures_by_lane
            ],
        )
    found += ["", "pushes, newest first"]
    found += table(
        [
            "start",
            "bead",
            "wall s",
            "fast check",
            "rebase and push",
            "cpu s",
            "queued s",
            "repeats",
            "lanes",
        ],
        [
            [
                p.start[:16],
                p.bead or "(no bead)",
                p.seconds,
                p.fast_check_seconds,
                p.rest_seconds,
                p.cpu_seconds,
                p.queued_seconds,
                p.repeats,
                ", ".join(lane.lane for lane in p.lanes) or "-",
            ]
            for p in f.rows
        ],
    )
    return found


FIELDS: dict[str, str] = {
    "pushes": "tool calls that ran `enact push` and started in the range; one run in the "
    "background ends when the session was told it finished",
    "split": "pushes whose fast check the run log could join: lane runs in the bead's "
    "worktree inside the call. Older run-log lines name no worktree",
    "median_wall_seconds": "median seconds of a push, from the call to its end",
    "median_fast_check_seconds": "median seconds of a split push covered by at least one "
    "fast-check lane run",
    "median_rest_seconds": "median seconds of a split push outside the fast check: rebase, "
    "waiting and push",
    "fast_check_seconds": "seconds of the push covered by at least one fast-check lane "
    "run; the lanes run at once, so their seconds divide this and are not added",
    "rest_seconds": "the push's seconds outside the fast check: rebase, waiting and push",
    "cpu_seconds": "CPU seconds of the push's fast-check lane runs, measured or estimated "
    "from wall seconds",
    "queued_seconds": "seconds the push's commands waited for CPU tokens",
    "checks": "fast-check runs inside the push, one per commit",
    "repeats": "fast-check runs after the first: the push lost the race to staging, "
    "rebased and checked again",
    "repeat_seconds": "seconds covered by the repeated runs",
    "repeat_cpu_seconds": "CPU seconds of the repeated runs",
    "failed": "whether the push's last fast-check run failed a check",
    "lanes": "check lanes with their runs, covered seconds and CPU seconds",
    "deferred": "fast-check work left to slow CI, from the push's output: steps cut at the "
    "hard limit, and tests cut to fit the time left",
    "failures": "the fast check's failures in the range by verdict",
    "failures_by_lane": "the fast check's failures in the range by check lane and verdict",
    "rows": "the newest pushes, each split",
}


def wall_points(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    return [(p.start, p.seconds) for p in pushes(db, facts, scope)]


def repeat_points(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    return [(p.start, float(p.repeats)) for p in pushes(db, facts, scope)]


register(
    Metric(
        "push.wall-seconds",
        "wall seconds of each push, from the call to its end",
        "seconds",
        0,
        wall_points,
    )
)
register(
    Metric(
        "push.repeats",
        "fast-check runs after the first in each push, from losing the race to staging",
        "runs",
        1,
        repeat_points,
    )
)
