"""The pruning rule (docs/project-50k.md, K18; Target architecture, Checks, "Pruning") as code:
`by ci-report --prune` prints each check the rule acts on, with its action and its numbers, for
the weekly prune to apply in one commit with no judgement of its own.

| Record                                        | Action              |
| --------------------------------------------- | ------------------- |
| Three stale updates and no catch              | Delete              |
| Quarantined, never caught anything            | Delete              |
| Quarantined, has caught a bug                 | Fix                 |
| Never failed, in the costliest tenth          | Run in slow CI only |
| A `keep` comment in its source, with a reason | Keep                |

Stale updates and flakes count over the report's window: the stale verdicts on the check's bead
and the rewrites the backfill logged (runlog.REWRITE), as by-w8gv.20.4 counted them when it
applied the rule by hand. A catch counts whenever it was: a check that once caught a bug is not
deleted for growing old. Quarantine is today's (verdicts.quarantine). The costliest tenth ranks
the tests the fast check could leave out (SLOW_ONLY_LANES) by their seconds per run in it, those
already in verdicts.SLOW_ONLY by the numbers they moved with, so that each week does not move
another tenth. Pending checks (verdicts.PENDING) are left out, as are checks that no longer run:
those missing from the latest slow-CI run of their lane.
"""

from __future__ import annotations

import json
import re
import subprocess
from collections.abc import Iterable
from dataclasses import asdict, dataclass, field
from datetime import datetime
from pathlib import Path

from . import paths, verdicts

STALE_TO_DELETE = 3
# The costliest tenth: the checks ranked first of every COSTLIEST_SHARE.
COSTLIEST_SHARE = 10
# The lanes whose checks checks.fast() can leave out by verdicts.SLOW_ONLY: a nextest filter
# for a Rust test (rust_tests.slow_only_filter). A lane joins when its fast step can.
SLOW_ONLY_LANES = ("rust",)

STALE = "three stale updates and no catch"
UNCAUGHT = "quarantined, never caught anything"
CAUGHT = "quarantined, has caught a bug"
COSTLY = "never failed, in the costliest tenth"
ROWS: dict[str, str] = {
    STALE: "delete",
    UNCAUGHT: "delete",
    CAUGHT: "fix",
    COSTLY: "slow-only",
}


@dataclass
class Record:
    """A check's numbers, as the rule reads them."""

    stale: int = 0
    catches: int = 0
    flakes: int = 0
    failures: int = 0
    runs: int = 0
    seconds: float = 0.0
    fast_runs: int = 0
    fast_seconds: float = 0.0
    quarantined: bool = False

    def per_run(self) -> float:
        return self.seconds / self.runs if self.runs else 0.0

    def fast_per_run(self) -> float:
        return self.fast_seconds / self.fast_runs if self.fast_runs else 0.0


@dataclass(frozen=True)
class Action:
    check: str
    action: str
    row: str
    record: Record = field(compare=False)
    keep: str = ""

    def line(self) -> str:
        r = self.record
        numbers = (
            f"stale {r.stale}, catch {r.catches}, flaky {r.flakes},"
            f" {r.failures} fails in {r.runs} runs, {r.per_run():.1f} s a run"
        )
        if self.row == COSTLY:
            numbers = (
                f"{r.fast_per_run():.1f} s a run over {r.fast_runs} fast-check runs"
            )
        line = f"{self.action:9}  {self.check}  ({numbers}): {self.row}"
        return line + (f"; keep: {self.keep}" if self.keep else "")

    def json(self) -> dict[str, object]:
        r = self.record
        return {
            "check": self.check,
            "action": self.action,
            "row": self.row,
            "keep": self.keep,
            **asdict(r),
            "seconds_per_run": round(r.per_run(), 3),
            "fast_seconds_per_run": round(r.fast_per_run(), 3),
            "seconds": round(r.seconds, 3),
            "fast_seconds": round(r.fast_seconds, 3),
        }


def current(records: Iterable[dict[str, object]]) -> dict[str, set[str]]:
    """Per lane, the checks of its latest slow-CI run that ran its checks: the commit of its
    latest observed pass, and every check logged at that commit."""
    ci = [r for r in records if r.get("script") == "ci" and not r.get("backfilled")]
    latest: dict[str, tuple[str, str]] = {}
    for r in ci:
        lane, when = str(r.get("lane")), str(r.get("time"))
        if r.get("outcome") == "pass" and when >= latest.get(lane, ("", ""))[0]:
            latest[lane] = (when, str(r.get("commit")))
    found: dict[str, set[str]] = {lane: set() for lane in latest}
    for r in ci:
        lane = str(r.get("lane"))
        if lane in latest and r.get("commit") == latest[lane][1]:
            found[lane].add(str(r.get("check")))
    return found


def records(
    log: Iterable[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    since: datetime,
) -> dict[str, Record]:
    found: dict[str, Record] = {}
    for r in log:
        if r.get("script") == "bisect":
            continue
        rec = found.setdefault(str(r.get("check")), Record())
        outcome = r.get("outcome")
        if outcome == "rewrite":
            rec.stale += 1
            continue
        if outcome not in ("pass", "fail"):
            continue
        seconds = r.get("seconds")
        took = float(seconds) if isinstance(seconds, (int, float)) else 0.0
        rec.runs += 1
        rec.seconds += took
        rec.failures += outcome == "fail"
        if r.get("script") == "check" and not r.get("backfilled"):
            rec.fast_runs += 1
            rec.fast_seconds += took
    for check, entries in history.items():
        rec = found.setdefault(check, Record())
        for e in entries:
            verdict = e.get("verdict")
            rec.catches += verdict == "catch"
            try:
                recent = datetime.fromisoformat(str(e.get("time"))) >= since
            except ValueError:
                continue
            rec.stale += recent and verdict == "stale"
            rec.flakes += recent and verdict == "flaky"
    for check in verdicts.quarantine(history):
        found.setdefault(check, Record()).quarantined = True
    return found


def lane(check: str) -> str:
    return check.split(":", 1)[0]


def costliest(found: dict[str, Record], running: set[str]) -> set[str]:
    """The costliest tenth of the tests the fast check runs, or ran before SLOW_ONLY took them
    out, by seconds per run in it."""
    cost = {
        c: r.fast_per_run()
        for c, r in found.items()
        if lane(c) in SLOW_ONLY_LANES and "::" in c and r.fast_runs and c in running
    }
    cost |= {s.check: s.seconds for s in verdicts.SLOW_ONLY}
    ranked = sorted(cost, key=lambda c: (-cost[c], c))
    return set(ranked[: len(ranked) // COSTLIEST_SHARE])


def row(check: str, r: Record, tenth: set[str], judged: bool) -> str:
    """The row of the rule's table a check's record falls in, if any."""
    if r.quarantined:
        return CAUGHT if r.catches else UNCAUGHT
    if r.stale >= STALE_TO_DELETE and not r.catches:
        return STALE
    if check in tenth and not r.failures and not judged:
        return COSTLY
    return ""


def prune(
    log: list[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    since: datetime,
    keeps: Keeps,
) -> list[Action]:
    """The rule's actions, a row at a time in the table's order, then by check."""
    found = records(log, history, since)
    running = {c for checks in current(log).values() for c in checks}
    tenth = costliest(found, running)
    moved = {s.check for s in verdicts.SLOW_ONLY}
    skipped = verdicts.pending_checks()
    actions: list[Action] = []
    for check, r in found.items():
        if check in skipped or check not in running:
            continue
        which = row(check, r, tenth, bool(history.get(check)))
        if not which or (which == COSTLY and check in moved):
            continue
        reason = keeps.reason(check)
        action = "keep" if reason else ROWS[which]
        actions.append(Action(check, action, which, r, reason))
    order = list(ROWS)
    return sorted(actions, key=lambda a: (order.index(a.row), a.check))


# `keep` comments.

KEEP: re.Pattern[str] = re.compile(r"//\s*keep:\s*(\S.*?)\s*$")
# How far below its `keep` comment a check's declaration may start.
KEEP_REACH = 12
TITLE: re.Pattern[str] = re.compile(
    r"\bcheck\(\s*(\"(?:[^\"\\]|\\.)*\"|'(?:[^'\\]|\\.)*'|`[^`]*`|\w+)"
)
TEST_FN: re.Pattern[str] = re.compile(r"\bfn\s+(\w+)")


@dataclass
class Keeps:
    """The checks a `keep` comment with a reason holds, each a pattern over check ids: beside a
    check() in ui/scripts/interactions.mjs (`// keep: ...` above it), a Rust test (above its
    `fn`, in the rust lane's side of verdicts.CHECK_SIDE), or a shot's `keep` in ui/shots.json.
    """

    found: list[tuple[re.Pattern[str], str]] = field(default_factory=list)

    @staticmethod
    def load() -> Keeps:
        keeps = Keeps()
        keeps.found += shot_keeps(paths.REPO / "ui" / "shots.json")
        grep = subprocess.run(
            [
                "git",
                "grep",
                "--untracked",
                "-n",
                "-I",
                "-E",
                "//[[:space:]]*keep:[[:space:]]*[^[:space:]]",
            ]
            + ["--", "ui/scripts/interactions.mjs", "rust/*.rs"],
            cwd=paths.REPO,
            capture_output=True,
            text=True,
            check=False,
        )
        hits: dict[str, list[int]] = {}
        for line in grep.stdout.splitlines():
            path, number, _ = line.split(":", 2)
            hits.setdefault(path, []).append(int(number))
        for path, numbers in hits.items():
            text = (paths.REPO / path).read_text(errors="replace")
            keeps.found += source_keeps(path, text, numbers)
        return keeps

    def reason(self, check: str) -> str:
        return next((why for p, why in self.found if p.fullmatch(check)), "")


def shot_keeps(path: Path) -> list[tuple[re.Pattern[str], str]]:
    try:
        shots = json.loads(path.read_text()).get("shots", [])
    except (OSError, json.JSONDecodeError, AttributeError):
        return []
    return [
        (re.compile(re.escape(f"shots:{s['name']}")), str(s["keep"]))
        for s in shots
        if isinstance(s, dict) and s.get("keep") and s.get("name")
    ]


def source_keeps(
    path: str, text: str, numbers: list[int]
) -> list[tuple[re.Pattern[str], str]]:
    """The checks the `keep` comments on lines `numbers` (1-based) of a source file hold."""
    lines = text.splitlines()
    tests = verdicts.regions(text, "tests") if path.endswith(".rs") else []
    rust_side = verdicts.matches(path, verdicts.CHECK_SIDE["rust"].globs)
    found: list[tuple[re.Pattern[str], str]] = []
    for n in numbers:
        reason = KEEP.search(lines[n - 1])
        if not reason:
            continue
        below = "\n".join(lines[n - 1 : n - 1 + KEEP_REACH])
        if path.endswith(".rs"):
            test = TEST_FN.search(below)
            if not test or not (rust_side or any(a <= n <= b for a, b in tests)):
                continue
            crate = Path(path).parts[2] if path.startswith("rust/crates/") else r"\w+"
            pattern = f"rust:{re.escape(crate)}(::.+)?::{re.escape(test[1])}"
        else:
            title = TITLE.search(below)
            if not title:
                continue
            pattern = "ui-test:" + title_pattern(title[1], text)
        found.append((re.compile(pattern), reason[1]))
    return found


def title_pattern(title: str, text: str) -> str:
    """A pattern over the titles a check() title expression gives: a string literal, a template
    whose `${name}` parts are the file's string constants or anything, or a constant's name.
    """
    if title[0] in "\"'":
        return re.escape(unescape(title[1:-1]))
    template = title[1:-1] if title[0] == "`" else f"${{{title}}}"
    pattern = ""
    for i, part in enumerate(re.split(r"\$\{([^}]*)\}", template)):
        if i % 2 == 0:
            pattern += re.escape(unescape(part))
            continue
        constant = re.search(
            rf"\bconst\s+{re.escape(part.strip())}\s*=\s*(\"(?:[^\"\\]|\\.)*\"|'(?:[^'\\]|\\.)*')",
            text,
        )
        pattern += re.escape(unescape(constant[1][1:-1])) if constant else ".+"
    return pattern


def unescape(literal: str) -> str:
    return re.sub(r"\\(.)", r"\1", literal)


def report(
    log: list[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    since: datetime,
    days: int,
    as_json: bool,
) -> int:
    actions = prune(log, history, since, Keeps.load())
    if as_json:
        print(
            json.dumps(
                {
                    "since": since.isoformat(timespec="seconds"),
                    "actions": [a.json() for a in actions],
                },
                indent=2,
            )
        )
        return 0
    print(
        f"ci-report --prune: {len(actions)} checks the pruning rule acts on over the last"
        f" {days} days (docs/project-50k.md, Checks, Pruning)"
    )
    for a in actions:
        print(a.line())
    return 0
