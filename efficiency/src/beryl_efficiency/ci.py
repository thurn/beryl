"""`just efficiency ci`: what CI costs and what each part of it buys
(docs/efficiency-dashboard.md, "CI accounting"). Checks form a tree, script, check lane,
family, check, and every level shows the same figures: CPU, runs and the share skipped, the
failures by verdict, CPU per catch, and the cost of flaky failures. Beside the tree is the CPU
by kind of process that the script's check lanes used, from the CPU sampler.

How the figures are made:

- CPU is the run log's measured CPU seconds. A line with no measurement (older lines, and
  Windows) is estimated from its wall seconds times its lane's measured CPU per wall second
  in the range, or every lane's where its lane has none, as `just ci-value` estimates it.
- Runs count lines that ran a check; a skipped line is counted apart. A script's runs are
  its distinct runs, one per host and commit, as `just ci-value` counts slow-CI runs.
- Failures are counted by the verdict recorded on the check's bead, under the script and
  lane that recorded it, in the range by the failure's time.
- The flake cost is one rerun per flaky failure, at the check's mean CPU and wall seconds per
  run, and beside it the dollars of the sessions that worked the beads labeled `flaky-check`
  for those checks, from those beads' windows in the range. The run log keeps each check per
  day added up, so a median per run cannot be had; the mean is shown instead.
- The CPU by kind of process joins the sampler's minutes for the script's context (`slow_ci`
  for slow CI, `fast_check` for the fast check) to the check-lane runs that overlap each
  minute, in the same worktree where both name one, sharing each minute among them by each
  run's CPU per second. CPU in no run's minute is `(no lane)`.
- For the oracle check lane, the scenarios that a run left out because an earlier run had
  recorded their outcome are not in the run log. They are estimated per engine as the lane's
  runs times the scenarios seen in the range, less those played or skipped.

Its metrics are `slow-ci.cpu-seconds-per-run`, `slow-ci.runs-per-day` and
`fast-check.cpu-seconds`.
"""

from __future__ import annotations

import math
import sqlite3
from collections import defaultdict
from dataclasses import asdict, dataclass, field

from .cilog import VERDICTS, family, utc
from .facts import Facts
from .metrics import Metric, Points, register
from .render import Result, describe, table
from .rollups import day_of, number
from .scope import Scope

# What each script is called.
SCRIPTS: dict[str, str] = {
    "ci": "slow CI",
    "check": "the fast check",
    "sweep": "the browser sweep",
    "nightly": "the nightly",
    "bisect": "bisection",
}
# The CPU sampler's context for a script's processes (tools/src/beryl_tools/cpu.py).
CONTEXTS: dict[str, str] = {"ci": "slow_ci", "check": "fast_check"}
# The titles flaky.py gives the beads it files to delete a flaky check.
FLAKY_TITLES: tuple[str, ...] = (
    "Delete flaky check ",
    "Delete the flaky test in step ",
)
FLAKY_LABEL = "flaky-check"
# How many checks a family's view lists before adding up the rest.
CHECK_ROWS = 40
NO_LANE = "(no lane)"
# What each field of the `ci` result means, which the page shows on hover.
FIELDS: dict[str, str] = {
    "script": "the script whose checks these are: ci (slow CI), check (the fast check), "
    "sweep, nightly or bisect",
    "level": "what the rows are: check lanes, families or checks",
    "script_runs": "the script's runs that started in the range: one per host and commit, "
    "and for the fast check one per worktree and tree",
    "days_from": "the first local day the check figures count: the run log is kept added "
    "up by day, so they count whole days",
    "total": "the figures of everything the query selected",
    "rows": "the tree's next level, costliest first; the oracle lane lists its engines",
    "core_hours": "CPU hours: the run log's measured CPU, with unmeasured lines estimated "
    "from their wall seconds",
    "estimated_percent": "percent of the CPU estimated from wall seconds",
    "runs": "runs of checks; a skipped check is counted apart",
    "skipped_percent": "percent of the checks' runs and skips that were skips",
    "cpu_seconds_per_run": "mean CPU seconds of one run of a check",
    "catch": "failures fixed by a change to product code: real bugs found",
    "stale": "failures fixed by a change to the check itself",
    "flaky": "failures that passed on a rerun of the same code",
    "mixed": "failures whose fix changed both the check and the product",
    "miss": "slow-CI failures of a check the fast check had not chosen",
    "core_hours_per_catch": "CPU hours per catch; null when nothing was caught, and the "
    "text shows the day of the first run",
    "first_run": "the local day of the earliest run in the store",
    "flake_cpu_seconds": "CPU seconds of one rerun per flaky failure, at the check's mean",
    "flake_wall_seconds": "wall seconds of one rerun per flaky failure, at the check's mean",
    "flake_dollars": "dollars of the sessions that worked the beads filed to delete the "
    "flaky checks, from their windows in the range",
    "kinds": "the CPU of the script's processes by kind, from the CPU sampler, with the "
    "oracle kind split by engine",
    "oracle": "oracle scenarios by engine: seen in the range, played, skipped, and left "
    "out because an earlier run recorded their outcome (an estimate)",
}
OTHERS = "(other checks)"
ORACLE = "oracle"


@dataclass
class Raw:
    """What a set of checks adds up to before it is rounded."""

    cpu: float = 0.0
    estimated: float = 0.0
    wall: float = 0.0
    runs: int = 0
    skips: int = 0
    verdicts: dict[str, int] = field(default_factory=dict)
    flake_cpu: float = 0.0
    flake_wall: float = 0.0
    flake_dollars: float = 0.0
    first: str | None = None

    def add(self, other: Raw) -> None:
        self.cpu += other.cpu
        self.estimated += other.estimated
        self.wall += other.wall
        self.runs += other.runs
        self.skips += other.skips
        for verdict, count in other.verdicts.items():
            self.verdicts[verdict] = self.verdicts.get(verdict, 0) + count
        self.flake_cpu += other.flake_cpu
        self.flake_wall += other.flake_wall
        self.flake_dollars += other.flake_dollars
        if other.first is not None and (self.first is None or other.first < self.first):
            self.first = other.first


@dataclass
class Node:
    """One node of the tree: a script, a check lane, a family or a check."""

    name: str
    core_hours: float
    estimated_percent: float
    runs: int
    skipped_percent: float | None
    cpu_seconds_per_run: float | None
    catch: int
    stale: int
    flaky: int
    mixed: int
    miss: int
    core_hours_per_catch: float | None
    first_run: str | None
    flake_cpu_seconds: float
    flake_wall_seconds: float
    flake_dollars: float
    children: list[Node] = field(default_factory=list)


@dataclass
class Kind:
    """The CPU of one kind of process, and for the oracle kind its split by engine."""

    kind: str
    core_hours: float
    percent: float
    engines: dict[str, float] = field(default_factory=dict)


@dataclass
class Engine:
    """One oracle engine's scenarios in the range: played, skipped, and left out because an
    earlier run recorded their outcome (an estimate)."""

    engine: str
    scenarios: int
    played: int
    skipped: int
    remembered: int


@dataclass
class Ci:
    since: str | None
    until: str | None
    range: str
    script: str
    lane: str | None
    family: str | None
    level: str
    days_from: str | None
    script_runs: int
    total: Node
    rows: list[Node]
    kinds: list[Kind]
    oracle: list[Engine]


def node(name: str, raw: Raw, children: list[Node] | None = None) -> Node:
    catches = raw.verdicts.get("catch", 0)
    tried = raw.runs + raw.skips
    return Node(
        name,
        round(raw.cpu / 3600, 2),
        round(100 * raw.estimated / raw.cpu, 1) if raw.cpu else 0.0,
        raw.runs,
        round(100 * raw.skips / tried, 1) if tried else None,
        round(raw.cpu / raw.runs, 2) if raw.runs else None,
        catches,
        raw.verdicts.get("stale", 0),
        raw.verdicts.get("flaky", 0),
        raw.verdicts.get("mixed", 0),
        raw.verdicts.get("miss", 0),
        round(raw.cpu / 3600 / catches, 2) if catches else None,
        raw.first,
        round(raw.flake_cpu, 1),
        round(raw.flake_wall, 1),
        round(raw.flake_dollars, 2),
        children or [],
    )


def days(scope: Scope) -> tuple[str, list[object]]:
    """A `WHERE` clause (starting with `AND`) that keeps the `checks` rows of the scope's
    local days and host."""
    clause = ""
    given: list[object] = []
    if scope.range.since:
        clause += " AND day >= ?"
        given.append(day_of(scope.range.since))
    if scope.range.until:
        clause += " AND day <= ?"
        given.append(day_of(scope.range.until))
    if scope.host:
        clause += " AND host = ?"
        given.append(scope.host)
    return clause, given


def bounds(column: str, scope: Scope) -> tuple[str, list[object]]:
    """A `WHERE` clause (starting with `AND`) that keeps `column` in the scope's range, and
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


@dataclass
class Ratios:
    """Measured CPU seconds per wall second, by lane and over every lane."""

    lanes: dict[str, float]
    overall: float

    def estimate(self, lane: str, wall: float) -> float:
        return wall * self.lanes.get(lane, self.overall)


def ratios(db: sqlite3.Connection, script: str, scope: Scope) -> Ratios:
    clause, given = days(scope)
    cpu: dict[str, float] = {}
    wall: dict[str, float] = {}
    for lane, measured, seconds in db.execute(
        f"""SELECT lane, sum(cpu_seconds), sum(measured_seconds) FROM checks
        WHERE script = ?{clause} GROUP BY lane""",
        (script, *given),
    ):
        cpu[str(lane)], wall[str(lane)] = float(measured), float(seconds)
    total = sum(wall.values(), 0.0)
    return Ratios(
        {lane: cpu[lane] / wall[lane] for lane in cpu if wall[lane] > 0},
        sum(cpu.values(), 0.0) / total if total > 0 else 1.0,
    )


@dataclass(frozen=True)
class Check:
    lane: str
    name: str


def flake_dollars(facts: Facts, scope: Scope) -> dict[str, float]:
    """The dollars of the windows in the range of the beads filed to delete each flaky
    check, by check."""
    windows = facts.bead_windows(scope)
    found: dict[str, float] = defaultdict(float)
    for b in facts.beads.values():
        if FLAKY_LABEL not in b.labels or not b.title:
            continue
        check = next(
            (b.title[len(t) :] for t in FLAKY_TITLES if b.title.startswith(t)), None
        )
        if check is None:
            continue
        for w in windows.get(b.id, []):
            if scope.range.holds(w.start):
                found[check] += w.total_dollars()
    return dict(found)


def checks(
    db: sqlite3.Connection, facts: Facts, scope: Scope, script: str
) -> dict[Check, Raw]:
    """Every check of `script` with runs or verdicts in the range, added up."""
    rates = ratios(db, script, scope)
    clause, given = days(scope)
    found: dict[Check, Raw] = {}
    for lane, name, runs, skips, seconds, cpu, measured in db.execute(
        f"""SELECT lane, "check", sum(runs), sum(skips), sum(seconds), sum(cpu_seconds),
            sum(measured_seconds)
        FROM checks WHERE script = ?{clause} GROUP BY lane, "check" """,
        (script, *given),
    ):
        estimated = rates.estimate(str(lane), float(seconds) - float(measured))
        found[Check(str(lane), str(name))] = Raw(
            float(cpu) + estimated,
            estimated,
            float(seconds),
            int(runs),
            int(skips),
        )
    clause, given = bounds("time", scope)
    for lane, name, verdict, count in db.execute(
        f"""SELECT lane, "check", verdict, count(*) FROM verdicts
        WHERE script = ?{clause} GROUP BY lane, "check", verdict""",
        (script, *given),
    ):
        raw = found.setdefault(Check(str(lane), str(name)), Raw())
        raw.verdicts[str(verdict)] = int(count)
    host = " AND host = ?" if scope.host else ""
    for lane, name, first in db.execute(
        f'SELECT lane, "check", min(day) FROM checks WHERE script = ?{host} '
        'GROUP BY lane, "check"',
        (script, *([scope.host] if scope.host else [])),
    ):
        raw = found.get(Check(str(lane), str(name)))
        if raw is not None:
            raw.first = str(first)
    dollars = flake_dollars(facts, scope)
    for check, raw in found.items():
        flaky = raw.verdicts.get("flaky", 0)
        if flaky and raw.runs:
            raw.flake_cpu = flaky * raw.cpu / raw.runs
            raw.flake_wall = flaky * raw.wall / raw.runs
        raw.flake_dollars = dollars.get(check.name, 0.0)
    return found


def added(raws: list[Raw]) -> Raw:
    found = Raw()
    for raw in raws:
        found.add(raw)
    return found


def grouped(found: dict[Check, Raw], key: str) -> dict[str, Raw]:
    """The checks added up by lane or by family, costliest first."""
    groups: dict[str, list[Raw]] = defaultdict(list)
    for check, raw in found.items():
        groups[check.lane if key == "lane" else family(check.name)].append(raw)
    sums = {name: added(raws) for name, raws in groups.items()}
    return dict(sorted(sums.items(), key=lambda kv: (-kv[1].cpu, kv[0])))


def run_key(script: str) -> str:
    """The columns of `lane_runs` that tell one run of `script` from another: a fast check
    is one tree in one worktree, and a run of any other script one commit on one host.
    """
    return 'host, worktree, "commit", tree' if script == "check" else 'host, "commit"'


def script_runs(db: sqlite3.Connection, scope: Scope, script: str) -> int:
    """The script's distinct runs that started in the range (`run_key`)."""
    clause, given = bounds("start", scope)
    found = db.execute(
        f"""SELECT count(*) FROM (SELECT DISTINCT {run_key(script)} FROM lane_runs
        WHERE script = ?{clause})""",
        (script, *given),
    ).fetchone()
    return int(found[0])


def minute_spans(start: float, end: float) -> range:
    return range(math.floor(start / 60), math.floor(end / 60) + 1)


def kinds_by_lane(
    db: sqlite3.Connection, scope: Scope, script: str, rates: Ratios
) -> dict[str, dict[str, float]]:
    """The CPU seconds of the script's processes by check lane and kind of process: each
    sampler minute shared among the check-lane runs that overlap it, by their CPU per
    second, in the same worktree where both name one."""
    context = CONTEXTS.get(script)
    if context is None:
        return {}
    # Runs by the minutes they overlap: (lane, worktree, start, end, CPU per second).
    buckets: dict[int, list[tuple[str, str, float, float, float]]] = defaultdict(list)
    clause, given = bounds("end", scope)
    for lane, worktree, start, end, seconds, cpu, measured in db.execute(
        f"""SELECT lane, worktree, start, end, seconds, cpu_seconds, measured_seconds
        FROM lane_runs WHERE script = ?{clause}""",
        (script, *given),
    ):
        began, ended = utc(start).timestamp(), utc(end).timestamp()
        spent = float(cpu) + rates.estimate(str(lane), float(seconds) - float(measured))
        rate = spent / max(ended - began, 1.0)
        for m in minute_spans(began, ended):
            buckets[m].append((str(lane), str(worktree), began, ended, rate))
    found: defaultdict[str, defaultdict[str, float]] = defaultdict(
        lambda: defaultdict(float)
    )
    clause, given = bounds("time", scope)
    for moment, length, kind, worktree, cpu in db.execute(
        f"""SELECT time, seconds, kind, worktree, cpu_seconds FROM cpu
        WHERE owner = ?{clause}""",
        (context, *given),
    ):
        began = utc(moment).timestamp()
        ended = began + float(length)
        weights: dict[str, float] = defaultdict(float)
        seen: set[tuple[str, str, float, float]] = set()
        for m in minute_spans(began, ended):
            for lane, where, start, end, rate in buckets.get(m, []):
                if (lane, where, start, end) in seen:
                    continue
                seen.add((lane, where, start, end))
                if worktree and where and worktree != where:
                    continue
                overlap = min(end, ended) - max(start, began)
                if overlap > 0 or (start == end and began <= start <= ended):
                    weights[lane] += rate * max(overlap, 1.0)
        whole = sum(weights.values())
        if whole <= 0:
            found[NO_LANE][str(kind)] += float(cpu)
            continue
        for lane, weight in weights.items():
            found[lane][str(kind)] += float(cpu) * weight / whole
    return {lane: dict(by_kind) for lane, by_kind in found.items()}


def kind_lines(
    by_kind: dict[str, float], engines: dict[str, float], whole_oracle: float
) -> list[Kind]:
    """The kinds of process, costliest first; the oracle kind split by engine in proportion
    to the oracle lane's CPU by engine."""
    total = sum(by_kind.values())
    found: list[Kind] = []
    for kind, seconds in sorted(by_kind.items(), key=lambda kv: (-kv[1], kv[0])):
        split = (
            {
                engine: round(seconds * share / whole_oracle / 3600, 2)
                for engine, share in engines.items()
            }
            if kind == ORACLE and whole_oracle > 0
            else {}
        )
        found.append(
            Kind(
                kind,
                round(seconds / 3600, 2),
                round(100 * seconds / total, 1) if total else 0.0,
                split,
            )
        )
    return found


def oracle_engines(
    db: sqlite3.Connection, scope: Scope, script: str, found: dict[Check, Raw]
) -> list[Engine]:
    clause, given = bounds("start", scope)
    runs = int(
        db.execute(
            f"SELECT count(*) FROM lane_runs WHERE script = ? AND lane = ?{clause}",
            (script, ORACLE, *given),
        ).fetchone()[0]
    )
    engines: dict[str, list[Raw]] = defaultdict(list)
    for check, raw in found.items():
        name = family(check.name)
        if check.lane == ORACLE and name.startswith("oracle:"):
            engines[name.split(":", 1)[1]].append(raw)
    result: list[Engine] = []
    for engine, raws in sorted(engines.items()):
        played = sum(r.runs for r in raws)
        skipped = sum(r.skips for r in raws)
        known = sum(1 for r in raws if r.runs or r.skips)
        result.append(
            Engine(
                engine, known, played, skipped, max(0, runs * known - played - skipped)
            )
        )
    return result


def report(
    db: sqlite3.Connection,
    facts: Facts,
    scope: Scope,
    script: str = "ci",
    lane: str | None = None,
    family_name: str | None = None,
) -> Result:
    found = checks(db, facts, scope, script)
    chosen = {
        c: r
        for c, r in found.items()
        if (lane is None or c.lane == lane)
        and (family_name is None or family(c.name) == family_name)
    }
    lanes = grouped(found, "lane")
    engine_cpu = {
        f.split(":", 1)[1]: r.cpu
        for f, r in grouped(
            {c: r for c, r in found.items() if c.lane == ORACLE}, "family"
        ).items()
        if f.startswith("oracle:")
    }
    if family_name is not None:
        level = "check"
        ordered = sorted(chosen.items(), key=lambda kv: (-kv[1].cpu, kv[0].name))
        rows = [node(c.name, r) for c, r in ordered[:CHECK_ROWS]]
        if len(ordered) > CHECK_ROWS:
            rows.append(node(OTHERS, added([r for _, r in ordered[CHECK_ROWS:]])))
    elif lane is not None:
        level = "family"
        rows = [node(f, r) for f, r in grouped(chosen, "family").items()]
    else:
        level = "lane"
        rows = []
        for name, raw in lanes.items():
            children = (
                [
                    node(f, r)
                    for f, r in grouped(
                        {c: r for c, r in found.items() if c.lane == name}, "family"
                    ).items()
                ]
                if name == ORACLE
                else []
            )
            rows.append(node(name, raw, children))
    by_lane = kinds_by_lane(db, scope, script, ratios(db, script, scope))
    by_kind: dict[str, float] = defaultdict(float)
    for name, kinds in by_lane.items():
        if lane is None or name == lane:
            for kind, seconds in kinds.items():
                by_kind[kind] += seconds
    total_name = family_name or lane or script
    result = Ci(
        scope.range.since,
        scope.range.until,
        scope.range.name,
        script,
        lane,
        family_name,
        level,
        day_of(scope.range.since) if scope.range.since else None,
        script_runs(db, scope, script),
        node(total_name, added(list(chosen.values()))),
        rows,
        kind_lines(dict(by_kind), engine_cpu, sum(engine_cpu.values())),
        (
            oracle_engines(db, scope, script, found)
            if lane in (None, ORACLE) and family_name is None
            else []
        ),
    )
    flags = scope.flags()
    more: list[str] = []
    if lane is None and family_name is None and rows:
        more.append(f"efficiency ci --script {script} --lane {rows[0].name} {flags}")
    elif family_name is None and rows:
        more.append(f"efficiency ci --script {script} --family {rows[0].name} {flags}")
    more += [
        "efficiency metrics slow-ci.cpu-seconds-per-run " + flags,
        "efficiency sql 'select * from verdicts order by time desc limit 20'",
    ]
    return Result(
        "ci",
        asdict(result),
        lines(result),
        [m.strip() for m in more],
        {**describe("since"), **FIELDS},
    )


def per_catch(n: Node) -> str:
    if n.core_hours_per_catch is not None:
        return f"{n.core_hours_per_catch:.2f}"
    return f"never since {n.first_run}" if n.first_run else "never"


def row(n: Node, indent: str = "") -> list[object]:
    return [
        indent + n.name,
        n.core_hours,
        n.runs,
        n.skipped_percent,
        n.catch,
        n.stale,
        n.flaky,
        n.mixed,
        n.miss,
        per_catch(n),
        n.flake_cpu_seconds,
        n.flake_dollars,
    ]


def lines(c: Ci) -> list[str]:
    where = " ".join(
        part
        for part in (
            f"--lane {c.lane}" if c.lane else "",
            f"--family {c.family}" if c.family else "",
        )
        if part
    )
    t = c.total
    found = [
        f"{SCRIPTS.get(c.script, c.script)} ({c.script})"
        + (f" {where}" if where else "")
        + f"  since {c.since or 'the start'} ({c.range})"
        + (f", checks from {c.days_from}" if c.days_from else ""),
        f"{c.script} runs {c.script_runs}   cpu {t.core_hours:.2f} core h "
        f"({t.estimated_percent:.1f}% estimated from wall seconds)   check runs {t.runs}"
        f"   skipped {number(t.skipped_percent, 1)}%",
        f"failures   catch {t.catch} | stale {t.stale} | flaky {t.flaky} | mixed {t.mixed}"
        f" | miss {t.miss}   core h per catch {per_catch(t)}",
        f"flake cost {t.flake_cpu_seconds:.1f} cpu s, {t.flake_wall_seconds:.1f} wall s "
        f"in reruns; {t.flake_dollars:.2f} dollars on beads to delete flaky checks",
        "",
    ]
    rows: list[list[object]] = []
    for n in c.rows:
        rows.append(row(n))
        rows += [row(child, "  ") for child in n.children]
    found += table(
        [
            c.level,
            "core h",
            "runs",
            "skip %",
            "catch",
            "stale",
            "flaky",
            "mixed",
            "miss",
            "core h per catch",
            "flake cpu s",
            "flake $",
        ],
        rows,
    )
    if c.oracle:
        found += ["", "oracle scenarios by engine (remembered: left out as recorded)"]
        found += table(
            ["engine", "scenarios", "played", "skipped", "remembered"],
            [
                [e.engine, e.scenarios, e.played, e.skipped, e.remembered]
                for e in c.oracle
            ],
        )
    found += ["", "cpu by kind of process"]
    if not c.kinds:
        found.append(
            "  none: the CPU sampler names no context for this script"
            if c.script not in CONTEXTS
            else "  none: no sampled minute in the range"
        )
    for k in c.kinds:
        split = ", ".join(f"{e} {h:.2f}" for e, h in k.engines.items())
        found.append(
            f"  {k.kind:<12} {k.core_hours:8.2f} core h  {k.percent:5.1f}%"
            + (f"   (Java: {split})" if split else "")
        )
    return found


def run_costs(
    db: sqlite3.Connection, scope: Scope, script: str, keys: str
) -> list[tuple[str, float]]:
    """Each run of `script` that started in the range, grouped by `keys`: its start and its
    CPU seconds, estimated where unmeasured."""
    rates = ratios(db, script, scope)
    clause, given = bounds("start", scope)
    runs: dict[tuple[object, ...], list[float | str]] = {}
    for found in db.execute(
        f"""SELECT {keys}, lane, start, seconds, cpu_seconds, measured_seconds
        FROM lane_runs WHERE script = ?{clause}""",
        (script, *given),
    ):
        count = len(keys.split(","))
        key = tuple(found[:count])
        lane, start, seconds, cpu, measured = found[count:]
        spent = float(cpu) + rates.estimate(str(lane), float(seconds) - float(measured))
        if key in runs:
            first, total = runs[key]
            runs[key] = [min(str(first), str(start)), float(total) + spent]
        else:
            runs[key] = [str(start), spent]
    return sorted((str(first), float(total)) for first, total in runs.values())


def slow_ci_cpu(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    return run_costs(db, scope, "ci", run_key("ci"))


def slow_ci_runs(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    counts: dict[str, list[str]] = defaultdict(list)
    for start, _cpu in run_costs(db, scope, "ci", run_key("ci")):
        counts[day_of(start)].append(start)
    return [(min(starts), float(len(starts))) for starts in counts.values()]


def fast_check_cpu(db: sqlite3.Connection, _: Facts, scope: Scope) -> Points:
    return run_costs(db, scope, "check", run_key("check"))


register(
    Metric(
        "slow-ci.cpu-seconds-per-run",
        "CPU seconds of each slow-CI run, one per host and commit",
        "cpu seconds",
        0,
        slow_ci_cpu,
    )
)
register(
    Metric(
        "slow-ci.runs-per-day",
        "slow-CI runs each day, one per host and commit",
        "runs",
        0,
        slow_ci_runs,
    )
)
register(
    Metric(
        "fast-check.cpu-seconds",
        "CPU seconds of each fast check: its check lanes' runs on one tree in one worktree",
        "cpu seconds",
        0,
        fast_check_cpu,
    )
)
