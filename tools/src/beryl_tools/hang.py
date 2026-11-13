"""Hang limits: how long a step of `by check` or `by ci` may run before it is killed as hung.

A timer in the checks is a safety net for a step that is stuck (a deadlock, a browser that never
answers), never a gate meaning "this took too long": a slow run files a bead (budget.py) and
fails nothing. So each step's limit sits far above anything a healthy run of it takes on a
loaded machine: [`FACTOR`] times its slowest pass in the last [`DAYS`] days, never under
[`FLOOR`], and [`UNKNOWN`] for a step with no pass on record (a new step, a new machine). The
limit counts load-adjusted seconds (clock.py) from the step's start, after its wait for CPU
tokens, so neither the load nor a wait for tokens brings a step closer to it.

Each pass is recorded here, the slowest one per step per day, in
`$BERYL_CACHE/ci/step-times.json`, this machine's record and never committed. The run log
(runlog.py) cannot say how long a step took: it records each check a step reports, not the
step.

$CHECK_HANG_LIMIT_S sets every step's limit, for a test or a machine that needs another.
"""

from __future__ import annotations

import json
import os
import tempfile
from datetime import date, timedelta
from pathlib import Path

from . import paths, system

# Several times the slowest pass, so that a step that reaches it is very likely stuck rather
# than slowed by load or a cold build.
FACTOR = 4
# The least limit: a step that passes in seconds when its build is warm still gets enough for a
# cold build on a machine where every core is busy. A cold `clippy` once took 555 s in slow CI.
FLOOR = 900.0
# The limit of a step with no pass on record, which may be a cold build of the whole workspace.
UNKNOWN = 1800.0
# The most limit, which keeps every step's limit under the wall limit before Enact's
# `slow_timeout` kills the run (bisect.time_left), so a hung step is named as hung.
CAP = 2400.0
DAYS = 30


def store_path() -> Path:
    return paths.cache_root() / "ci" / "step-times.json"


def key(script: str, lane: str, step: str) -> str:
    return f"{script} {lane}:{step}"


class Limits:
    """The hang limits of one script's steps, read once when the run starts."""

    def __init__(self, script: str) -> None:
        self.script = script
        self.slowest: dict[str, float] = {}
        since = (date.today() - timedelta(days=DAYS)).isoformat()
        for name, days in read().items():
            recent = [s for day, s in days.items() if day >= since]
            if recent:
                self.slowest[name] = max(recent)

    def limit(self, lane: str, step: str) -> tuple[float, str]:
        """The step's limit in adjusted seconds and how it was set, for the log."""
        forced = os.environ.get("CHECK_HANG_LIMIT_S")
        if forced:
            return float(forced), "$CHECK_HANG_LIMIT_S"
        slowest = self.slowest.get(key(self.script, lane, step))
        if slowest is None:
            return UNKNOWN, f"no pass in {DAYS} days"
        limit = min(CAP, max(FLOOR, FACTOR * slowest))
        return limit, f"{FACTOR}x its slowest pass in {DAYS} days, {slowest:.0f} s"

    def passed(self, lane: str, step: str, seconds: float) -> None:
        """Records a pass of the step that took `seconds` (adjusted, waits left out). Recording
        never fails a check."""
        try:
            record(key(self.script, lane, step), seconds)
        except Exception as e:
            print(f"hang limits: not recorded: {e}")


def read() -> dict[str, dict[str, float]]:
    """Each step's slowest pass per day, by step and then by ISO date."""
    try:
        data = json.loads(store_path().read_text())
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def record(name: str, seconds: float) -> None:
    path = store_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    with system.file_lock(path.with_suffix(".lock")):
        data = read()
        since = (date.today() - timedelta(days=DAYS)).isoformat()
        days = {d: s for d, s in data.get(name, {}).items() if d >= since}
        today = date.today().isoformat()
        days[today] = round(max(seconds, days.get(today, 0.0)), 1)
        data[name] = days
        # Written whole and renamed into place, so a reader never sees half of it.
        handle, tmp = tempfile.mkstemp(dir=path.parent, suffix=".json")
        with os.fdopen(handle, "w") as f:
            json.dump(data, f, indent=0, sort_keys=True)
        os.replace(tmp, path)
