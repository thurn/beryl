"""The fast check's time contract, shared by the run (checks.py) and the steps that choose how
much of their work to do (the browser lanes in selection.py, the Rust tests in rust_tests.py and
the oracles in oracles.py).

The fast check runs before every push, so it has to stay short. Before this contract, only the
browser lanes fitted their work to a time box: the Rust tests, the oracles and the merge queue's
second check ran whatever a change selected, and a push could take 25 minutes to land. Slow CI
on staging is the gate that keeps `master` green; the fast check only has to catch the likely
breaks quickly and leave the rest to slow CI.

- [`TARGET`]: the run should finish in 60 seconds on the reference machine with nothing else
  running (the checks' load-adjusted clock, clock.py).
- [`LOADED_TARGET`]: however loaded the machine is, the run should finish in 90 wall seconds.
  Under load, the steps that can choose plan for less work rather than stretching past it.
- [`HARD_LIMIT`]: at 180 wall seconds the run kills any step still running. The step and the
  rest of its lane are deferred to slow CI, and the run passes on what it did check.

A step that can choose reads [`left`]: the wall seconds until the target, as the load is now.
"""

from __future__ import annotations

import os
import threading
import time
from datetime import datetime, timedelta, timezone
from typing import TYPE_CHECKING

from . import speed, system
from .clock import stretch

if TYPE_CHECKING:
    from .selection import Record

TARGET = 60.0
LOADED_TARGET = 90.0
HARD_LIMIT = 180.0
# When the run started (time.time()), set by the fast check for the steps it starts.
STARTED_ENV = "BERYL_CHECK_STARTED"


def target_wall(load: float, cpus: int, factor: float) -> float:
    """The wall seconds the run has under `load` on `cpus` cores, on a machine `factor` times
    slower than the reference: [`TARGET`] stretched as the clock stretches it, but never past
    [`LOADED_TARGET`]."""
    return min(LOADED_TARGET, TARGET * stretch(load, cpus) * factor)


def started() -> float | None:
    """When the fast check that started this process began, or None outside one."""
    try:
        return float(os.environ[STARTED_ENV])
    except (KeyError, ValueError):
        return None


def left(now: float | None = None) -> float | None:
    """The wall seconds left until the run's target, as the load is now; None outside a fast
    check, for a step run by hand or by slow CI, which choose for themselves."""
    begun = started()
    if begun is None:
        return None
    cpus = os.cpu_count() or 1
    wall = target_wall(system.loadavg()[0], cpus, speed.factor())
    return wall - ((time.time() if now is None else now) - begun)


def env() -> dict[str, str]:
    """The variable that starts the contract's clock for the steps of a fast check."""
    return {STARTED_ENV: repr(time.time())}


class Records:
    """The records (selection.Record) of a lane's checks from the run log, read in a thread
    while the step builds what it runs: reading 30 days of the log takes about 10 seconds.
    """

    def __init__(self, lane: str) -> None:
        self.lane = lane
        self.known: dict[str, Record] = {}
        self.thread = threading.Thread(target=self.read, daemon=True)
        self.thread.start()

    def read(self) -> None:
        # They import this module, by way of checks.py.
        from . import runlog, verdicts
        from .selection import RECORD_DAYS, records

        since = datetime.now(timezone.utc) - timedelta(days=RECORD_DAYS)
        try:
            log = [r for r in runlog.read_log(since) if r.get("lane") == self.lane]
            self.known = records(log, verdicts.cached_history(), None, since)
        except Exception as e:  # A plan without records still fits, at default times.
            print(f"{self.lane}: no records to plan with: {e}", flush=True)

    def get(self) -> dict[str, Record]:
        self.thread.join()
        return self.known
