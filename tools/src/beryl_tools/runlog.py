"""The run log (docs/project-50k.md, K18): every run of every check `by check` and `by ci` make
appends one JSON line to `$BERYL_CACHE/ci/runs.jsonl`, under a file lock, since worktrees and
slow CI write at once. It is this machine's record, never committed. `by ci-report` sums it.

A check is the finest thing a step can report: one Rust test (`rust:<binary>::<test path>`), one
browser check (`ui-test:<title>`), one screenshot (`shots:<name>`), one oracle scenario
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
from collections.abc import Iterable, Iterator
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import paths, system

# The file a step appends its checks' results to, one JSON object a line: check, outcome, seconds.
RESULTS_ENV = "BERYL_CHECK_RESULTS"
OUTCOMES = ("pass", "fail", "skip")


def log_path() -> Path:
    return paths.cache_root() / "ci" / "runs.jsonl"


def warn(e: BaseException) -> None:
    print(f"run log: not written: {e}", file=sys.stderr)


def result(check: str, outcome: str, seconds: float) -> None:
    """Reports one check's result to the run log, when a run asked for results; a no-op outside
    `by check` and `by ci`."""
    path = os.environ.get(RESULTS_ENV)
    if not path:
        return
    line = {"check": check, "outcome": outcome, "seconds": round(seconds, 3)}
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


@dataclass
class Run:
    """One `by check` or `by ci`: what every record of it shares."""

    script: str
    commit: str = ""
    tree: str = ""
    host: str = field(default_factory=platform.node)
    chosen: str = "all"

    @staticmethod
    def start(script: str) -> Run:
        try:
            return Run(script, git("rev-parse", "HEAD"), tree_hash())
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
        self, lane: str, step: str, passed: bool, seconds: float, results: Path
    ) -> list[dict[str, object]]:
        """A finished step's records: one per check it reported, and one for the step itself
        when it reported none, or failed though none of them did."""
        now = datetime.now(timezone.utc).isoformat(timespec="seconds")
        load = round(system.loadavg()[0], 2)
        checks = read_results(results)
        if not checks or (not passed and all(c["outcome"] != "fail" for c in checks)):
            checks.append(
                {
                    "check": f"{lane}:{step}",
                    "outcome": "pass" if passed else "fail",
                    "seconds": round(seconds, 3),
                }
            )
        return [
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
                "chosen": self.chosen,
            }
            for c in checks
        ]

    def log(
        self, lane: str, step: str, passed: bool, seconds: float, results: Path
    ) -> None:
        try:
            append(self.records(lane, step, passed, seconds, results))
        except Exception as e:  # Logging never fails a check.
            warn(e)


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

    def add(self, r: dict[str, object]) -> None:
        outcome = r.get("outcome")
        if outcome == "skip":
            self.skips += 1
            return
        self.runs += 1
        self.failures += outcome == "fail"
        seconds = r.get("seconds")
        self.seconds += seconds if isinstance(seconds, (int, float)) else 0.0
        verdict = r.get("verdict")
        if isinstance(verdict, str):
            self.verdicts[verdict] = self.verdicts.get(verdict, 0) + 1

    def json(self) -> dict[str, object]:
        return {
            "runs": self.runs,
            "failures": self.failures,
            "skips": self.skips,
            "seconds": round(self.seconds, 1),
            "verdicts": self.verdicts,
        }


def tally(
    records: Iterable[dict[str, object]],
) -> tuple[dict[str, Tally], dict[str, Tally]]:
    """Per check and per lane, costliest first."""
    checks: dict[str, Tally] = {}
    lanes: dict[str, Tally] = {}
    for r in records:
        checks.setdefault(str(r.get("check")), Tally()).add(r)
        lanes.setdefault(str(r.get("lane")), Tally()).add(r)

    def costliest(t: dict[str, Tally]) -> dict[str, Tally]:
        return dict(sorted(t.items(), key=lambda kv: (-kv[1].seconds, kv[0])))

    return costliest(checks), costliest(lanes)


def report(args: argparse.Namespace) -> int:
    since = datetime.now(timezone.utc) - timedelta(days=args.days)
    checks, lanes = tally(read_log(since))
    if args.json:
        print(
            json.dumps(
                {
                    "since": since.isoformat(timespec="seconds"),
                    "log": str(log_path()),
                    "checks": {k: t.json() for k, t in checks.items()},
                    "lanes": {k: t.json() for k, t in lanes.items()},
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
    for title, rows in (("check", checks), ("lane", lanes)):
        print(f"\n{'seconds':>9} {'runs':>6} {'fails':>6}  {title}  verdicts")
        for name, t in rows.items():
            verdicts = ", ".join(f"{v} {n}" for v, n in sorted(t.verdicts.items()))
            row = f"{t.seconds:9.1f} {t.runs:6} {t.failures:6}  {name}  {verdicts}"
            print(row.rstrip())
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "ci-report", help="each check's runs, failures and seconds from the run log"
    )
    p.add_argument("--days", type=int, default=30, help="how far back (default 30)")
    p.add_argument("--json", action="store_true", help="JSON instead of a table")
    p.set_defaults(func=report)
