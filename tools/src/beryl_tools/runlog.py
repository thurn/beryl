"""The run log: every run of every check `by check` and `by ci` make
appends one JSON line to `$BERYL_CACHE/ci/runs.jsonl`, under a file lock, since worktrees and
slow CI write at once. It is this machine's record, never committed. `by ci-report` sums it,
with the verdicts on each check's failures that verdicts.py records in beads.

A check is the finest thing a step can report: one Rust test (`rust:<binary>::<test path>`), one
browser check (`ui-test:<title>`, `verb:<verb>/<seat>/<layout>` for one of the stage's verbs,
or `shape:<shape>/<seat>/<layout>` for one of its prompt shapes), one screenshot (`shots:<name>`), one oracle scenario
(`oracle:<card>/<scenario>/<engine>`). A step reports them by appending lines to the file
`$BERYL_CHECK_RESULTS` names ([`result`] here, `recordResult` in ui/scripts/capture.mjs); a step
that reports none, such as fmt or clippy, is one check, `<lane>:<step>`, as is a step that failed
though none of its checks did (a build error, a page error no check owns).

Logging never fails a check: a log that cannot be written is reported and the run goes on.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
from collections.abc import Iterable, Iterator, Mapping
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import flaky, nightly, paths, prune, system, verdicts

# The file a step appends its checks' results to, one JSON object a line: check, outcome, seconds.
RESULTS_ENV = "BERYL_CHECK_RESULTS"
OUTCOMES = ("pass", "fail", "skip")
# A record backfilled from history (`backfilled`) of a commit that rewrote a check after the
# commit that added it: no run, but one stale update in the check's record (K18's prune).
REWRITE = "rewrite"
# The script of a slow-CI failure's bisection reruns (bisect.py).
BISECT = "bisect"
# What a check on the pending list (verdicts.PENDING) logs instead of a pass or a fail.
PENDING_OUTCOMES = {"pass": "pending-pass", "fail": "pending-fail"}


def log_path() -> Path:
    return paths.cache_root() / "ci" / "runs.jsonl"


def warn(e: BaseException) -> None:
    print(f"run log: not written: {e}", file=sys.stderr)


def result(check: str, outcome: str, seconds: float, chosen: str | None = None) -> None:
    """Reports one check's result to the run log, when a run asked for results, with why it
    ran when the step chose it (`chosen`); a no-op outside `by check` and `by ci`."""
    path = os.environ.get(RESULTS_ENV)
    if not path:
        return
    line: dict[str, object] = {
        "check": check,
        "outcome": outcome,
        "seconds": round(seconds, 3),
    }
    if chosen is not None:
        line["chosen"] = chosen
    try:
        with open(path, "a") as f:
            f.write(json.dumps(line) + "\n")
    except OSError as e:
        warn(e)


def git(*args: str, env: dict[str, str] | None = None) -> str:
    run = subprocess.run(
        ["git", *args],
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )
    return run.stdout.strip() if run.returncode == 0 else ""


def tree_hash() -> str:
    """The tree of the working tree as it is, untracked files included and ignored ones not, so
    an unchanged rerun is recognizable before anything is committed. It stages into a copy of
    the index, leaving the real one alone; empty when git cannot say."""
    index = git("rev-parse", "--path-format=absolute", "--git-path", "index")
    with tempfile.TemporaryDirectory() as tmp:
        copy = Path(tmp) / "index"
        if index and Path(index).exists():
            shutil.copyfile(index, copy)
        env = {**os.environ, "GIT_INDEX_FILE": str(copy)}
        if subprocess.run(
            ["git", "add", "--all", "--", "."],
            cwd=paths.REPO,
            env=env,
            capture_output=True,
            check=False,
        ).returncode:
            return ""
        return git("write-tree", env=env)


def read_results(path: Path) -> list[dict[str, object]]:
    """The checks a step reported, skipping lines that are not one."""
    found: list[dict[str, object]] = []
    try:
        lines = path.read_text(errors="replace").splitlines()
    except OSError:
        return found
    for line in lines:
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(r, dict)
            and isinstance(r.get("check"), str)
            and r.get("outcome") in OUTCOMES
            and isinstance(r.get("seconds"), (int, float))
        ):
            found.append(r)
    return found


def label(path: Path, chosen: Mapping[str, str], cut: Iterable[str]) -> None:
    """Writes into a step's results why each check ran (`chosen`, selection.py), and a `skip`
    for each one it chose but `cut`; a check it ran though it chose none keeps the run's.
    """
    try:
        results = read_results(path)
        for r in results:
            if r["check"] in chosen:
                r["chosen"] = chosen[str(r["check"])]
        for c in cut:
            results.append(
                {"check": c, "outcome": "skip", "seconds": 0.0, "chosen": "cut"}
            )
        path.write_text("".join(json.dumps(r) + "\n" for r in results))
    except OSError as e:
        warn(e)


def place(script: str) -> str:
    """Where a run's checks ran, so a pass settles only failures from the same place
    (verdicts.settle): the branch, or for slow CI or the browser sweep on a detached staging tip,
    `staging`, else the
    working tree's path."""
    branch = git("symbolic-ref", "--short", "-q", "HEAD")
    if branch:
        return branch
    return (
        "staging" if script in ("ci", "sweep") else git("rev-parse", "--show-toplevel")
    )


@dataclass
class Run:
    """One `by check` or `by ci`: what every record of it shares, and its records so far."""

    script: str
    commit: str = ""
    tree: str = ""
    place: str = ""
    host: str = field(default_factory=platform.node)
    # Why a check ran, unless its step says (`label`): slow CI runs them all.
    chosen: str = "all"
    logged: list[dict[str, object]] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)

    @staticmethod
    def start(script: str) -> Run:
        try:
            return Run(script, git("rev-parse", "HEAD"), tree_hash(), place(script))
        except Exception as e:  # Logging never fails a check.
            warn(e)
            return Run(script)

    @contextmanager
    def step(self, env: dict[str, str]) -> Iterator[tuple[dict[str, str], Path]]:
        """A step's environment, asking for its checks' results, and the file they go to."""
        handle, name = tempfile.mkstemp(prefix="beryl-check-", suffix=".jsonl")
        os.close(handle)
        path = Path(name)
        try:
            yield {**env, RESULTS_ENV: name}, path
        finally:
            path.unlink(missing_ok=True)

    def records(
        self,
        lane: str,
        step: str,
        passed: bool,
        seconds: float,
        results: Path,
        killed: str | None = None,
        cpu: float | None = None,
    ) -> list[dict[str, object]]:
        """A finished step's records: one per check it reported, and one for the step itself
        (marked `whole`) when it reported none, or failed though none of them did. A step that
        was killed, or exited `CANNOT_RUN`, marks its failures `timeout` with why (`hang`,
        `wall`, `stopped` or `cannot-run`, checks.lane_steps): neither says anything about a
        check, so none of them waits for a verdict (verdicts.settle).

        `cpu` is the CPU seconds of the step's whole process tree, which each record gets a share
        of as `cpu_seconds` (none where it is None, as on Windows). A step that ran many checks
        has no per-check measurement, so its CPU time is split among them in proportion to their
        wall `seconds`, evenly if all of those are 0; a skipped check gets none.
        """
        now = datetime.now(timezone.utc).isoformat(timespec="seconds")
        load = round(system.loadavg()[0], 2)
        checks = read_results(results)
        if not checks or (not passed and all(c["outcome"] != "fail" for c in checks)):
            checks.append(
                {
                    "check": f"{lane}:{step}",
                    "outcome": "pass" if passed else "fail",
                    "seconds": round(seconds, 3),
                    "whole": True,
                }
            )
        pending = verdicts.pending_checks()
        for c in checks:
            if c["check"] in pending:
                c["outcome"] = PENDING_OUTCOMES.get(str(c["outcome"]), c["outcome"])
        ran: list[dict[str, object]] = [c for c in checks if c["outcome"] != "skip"]
        weight: float = sum(float(str(c["seconds"])) for c in ran)

        def share(c: dict[str, object]) -> float:
            if cpu is None or c["outcome"] == "skip":
                return 0.0
            if weight <= 0:
                return cpu / len(ran)
            return cpu * float(str(c["seconds"])) / weight

        records: list[dict[str, object]] = [
            {
                "time": now,
                "host": self.host,
                "commit": self.commit,
                "tree": self.tree,
                "script": self.script,
                "lane": lane,
                "check": c["check"],
                "outcome": c["outcome"],
                "seconds": c["seconds"],
                "load": load,
                "chosen": c.get("chosen", self.chosen),
            }
            for c in checks
        ]
        for r, c in zip(records, checks):
            if cpu is not None:
                r["cpu_seconds"] = round(share(c), 3)
            if c.get("whole"):
                r["whole"] = True
        if killed:
            for r in records:
                if r["outcome"] == "fail":
                    r["timeout"] = killed
        return records

    def log(
        self,
        lane: str,
        step: str,
        passed: bool,
        seconds: float,
        results: Path,
        killed: str | None = None,
        cpu: float | None = None,
    ) -> list[dict[str, object]]:
        """Logs a finished step's records and returns them (`records`)."""
        try:
            records = self.records(lane, step, passed, seconds, results, killed, cpu)
        except Exception as e:  # Logging never fails a check.
            warn(e)
            return []
        with self.lock:
            self.logged += records
        try:
            append(records)
        except Exception as e:
            warn(e)
        return records

    def settle(self) -> list[str]:
        """The verdicts this run reached (verdicts.settle), and the delete beads its flakes
        filed (flaky.py), a line each."""
        with self.lock:
            records = list(self.logged)
        try:
            lines = verdicts.settle(
                records, self.script, self.place, self.tree, self.commit, self.host
            )
            flakes = {
                str(r["check"]): flaky.Flake(str(r["lane"]), bool(r.get("whole")))
                for r in records
                if r.get("flaky")
            }
            return lines + flaky.report(flakes, self.script, self.commit)
        except Exception as e:  # Neither do verdicts.
            warn(e)
            return []


def append(records: Iterable[dict[str, object]]) -> None:
    text = "".join(json.dumps(r) + "\n" for r in records)
    path = log_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    with system.file_lock(path.with_suffix(".lock")):
        with open(path, "a") as f:
            f.write(text)


def read_log(since: datetime) -> list[dict[str, object]]:
    """The records from `since` on, skipping any line that is not one."""
    try:
        lines = log_path().read_text(errors="replace").splitlines()
    except FileNotFoundError:
        return []
    records: list[dict[str, object]] = []
    for line in lines:
        try:
            r = json.loads(line)
            when = datetime.fromisoformat(r["time"])
        except (json.JSONDecodeError, KeyError, TypeError, ValueError):
            continue
        if when >= since:
            records.append(r)
    return records


@dataclass
class Tally:
    runs: int = 0
    failures: int = 0
    skips: int = 0
    seconds: float = 0.0
    verdicts: dict[str, int] = field(default_factory=dict)
    backfilled: int = 0

    def add(self, r: dict[str, object]) -> None:
        outcome = r.get("outcome")
        self.backfilled += bool(r.get("backfilled"))
        if outcome == REWRITE:
            self.judged("stale")
            return
        if outcome == "skip":
            self.skips += 1
            return
        self.runs += 1
        self.failures += outcome == "fail"
        seconds = r.get("seconds")
        self.seconds += seconds if isinstance(seconds, (int, float)) else 0.0

    def judged(self, verdict: str, backfilled: bool = False) -> None:
        self.verdicts[verdict] = self.verdicts.get(verdict, 0) + 1
        self.backfilled += backfilled

    def json(self) -> dict[str, object]:
        return {
            "runs": self.runs,
            "failures": self.failures,
            "skips": self.skips,
            "seconds": round(self.seconds, 1),
            "verdicts": self.verdicts,
            "backfilled": self.backfilled,
        }


def tally(
    records: Iterable[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    since: datetime,
) -> tuple[dict[str, Tally], dict[str, Tally]]:
    """Per check and per lane, costliest first: runs and seconds from the run log, verdicts
    since `since` from the checks' beads (verdicts.py). A bisection's reruns at older commits
    (bisect.py, script `bisect`) say nothing of the check's own record and are left out.
    """
    checks: dict[str, Tally] = {}
    lanes: dict[str, Tally] = {}
    for r in records:
        if r.get("script") == BISECT:
            continue
        checks.setdefault(str(r.get("check")), Tally()).add(r)
        lanes.setdefault(str(r.get("lane")), Tally()).add(r)
    for check, found in history.items():
        for e in found:
            try:
                recent = datetime.fromisoformat(str(e.get("time"))) >= since
            except ValueError:
                continue
            if recent:
                lane = str(e.get("lane") or check.split(":")[0])
                old = bool(e.get("backfilled"))
                checks.setdefault(check, Tally()).judged(str(e["verdict"]), old)
                lanes.setdefault(lane, Tally()).judged(str(e["verdict"]), old)

    def costliest(t: dict[str, Tally]) -> dict[str, Tally]:
        return dict(sorted(t.items(), key=lambda kv: (-kv[1].seconds, kv[0])))

    return costliest(checks), costliest(lanes)


def landed(since: datetime) -> int:
    """The commits that reached master from `since` on, merges aside."""
    count = git(
        "rev-list",
        "--count",
        "--no-merges",
        f"--since={since.isoformat()}",
        "origin/master",
    )
    return int(count) if count.isdigit() else 0


def misses(lanes: dict[str, Tally], since: datetime) -> dict[str, object]:
    """The selection misses (verdicts.py) since `since`, with the commits that landed meanwhile
    and the misses per 100 of them: the epic's measure for moving the browser lanes to nightly.
    """
    count = sum(t.verdicts.get("miss", 0) for t in lanes.values())
    commits = landed(since)
    rate = round(100 * count / commits, 2) if commits else None
    return {"misses": count, "landed": commits, "per_100": rate}


def report(args: argparse.Namespace) -> int:
    since = datetime.now(timezone.utc) - timedelta(days=args.days)
    records = read_log(since)
    if args.pending:
        return pending_report(records, args.json)
    store = verdicts.STORE
    history = store.history(store.checks())
    verdicts.save_history(history)
    if args.prune:
        return prune.report(records, history, since, args.days, args.json)
    checks, lanes = tally(records, history, since)
    missed = misses(lanes, since)
    if args.json:
        print(
            json.dumps(
                {
                    "coverage": nightly.coverage_age_line(),
                    "since": since.isoformat(timespec="seconds"),
                    "log": str(log_path()),
                    "checks": {k: t.json() for k, t in checks.items()},
                    "lanes": {k: t.json() for k, t in lanes.items()},
                    "misses": missed,
                },
                indent=2,
            )
        )
        return 0
    runs = sum(t.runs for t in lanes.values())
    print(
        f"ci-report: {runs} runs of {len(checks)} checks in the last {args.days} days"
        f" ({log_path()})"
    )
    print(nightly.coverage_age_line())
    rate = missed["per_100"]
    print(
        f"ci-report: {missed['misses']} selection misses in {missed['landed']} landed commits"
        + (f", {rate:.2f} per 100" if rate is not None else "")
    )
    for title, rows in (("check", checks), ("lane", lanes)):
        print(f"\n{'seconds':>9} {'runs':>6} {'fails':>6}  {title}  verdicts")
        for name, t in rows.items():
            judged = ", ".join(f"{v} {n}" for v, n in sorted(t.verdicts.items()))
            if t.backfilled:
                judged += f" ({t.backfilled} backfilled)"
            row = f"{t.seconds:9.1f} {t.runs:6} {t.failures:6}  {name}  {judged}"
            print(row.rstrip())
    return 0


def pending_report(records: list[dict[str, object]], as_json: bool) -> int:
    """The pending list, each entry with its check's latest outcome in the log."""
    last = {str(r.get("check")): str(r.get("outcome")) for r in records}
    rows = [
        {
            "check": p.check,
            "board": p.board,
            "bead": p.bead,
            "last": last.get(p.check, "not run"),
        }
        for p in verdicts.PENDING
    ]
    if as_json:
        print(json.dumps({"pending": rows}, indent=2))
        return 0
    passing = sum(r["last"] == "pending-pass" for r in rows)
    print(
        f"ci-report: {len(rows)} pending checks, {passing} now passing: the bead that makes"
        " one pass removes it from verdicts.PENDING"
    )
    for r in rows:
        now = "  NOW PASSES" if r["last"] == "pending-pass" else ""
        print(f"{r['check']}  board {r['board']}  bead {r['bead']}  {r['last']}{now}")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "ci-report", help="each check's runs, failures and seconds from the run log"
    )
    p.add_argument("--days", type=int, default=30, help="how far back (default 30)")
    p.add_argument("--json", action="store_true", help="JSON instead of a table")
    which = p.add_mutually_exclusive_group()
    which.add_argument(
        "--pending",
        action="store_true",
        help="only the pending list, with each check's latest outcome",
    )
    which.add_argument(
        "--prune",
        action="store_true",
        help="each check the pruning rule acts on, with its action and numbers",
    )
    p.set_defaults(func=report)
