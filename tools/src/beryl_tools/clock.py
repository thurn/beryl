"""The checks' clock (checks.py): seconds as the reference machine, with nothing else running,
would have spent them. Budgets and deadlines hold a run to what it would take there, whether
other work is slowing this machine down or this machine is slower than the reference
(speed.py)."""

from __future__ import annotations

import os
import threading
import time

from . import speed, system

# The most a busy machine slows a run's clock (`Clock`): a step that hangs dies by this many
# times its deadline, however loaded the machine.
MAX_STRETCH = 8.0


def stretch(load: float, cpus: int) -> float:
    """How many times longer than on a machine to itself work takes under `load` (the run
    queue's average length) on `cpus` cores: the cores are shared among that many threads,
    so a step gets about `cpus / load` of what it would, however many threads it runs (up to
    the cores). Its own threads count too, so a check that alone oversubscribes the machine
    gets back at most that much.
    """
    return min(MAX_STRETCH, max(1.0, load / cpus))


class Clock:
    """Seconds since it started, as the reference machine with nothing else running would have
    spent them: each stretch of wall time divided by how much the load then slowed it
    (`stretch`) and by how much slower this machine is than the reference
    (`speed.factor`). It reads the load as it is asked the time, which a running step does
    every half second."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.cpus: int = os.cpu_count() or 1
        # This machine's time for a fixed workload over the reference's (speed.py).
        self.speed: float = speed.factor()
        # perf_counter, not monotonic: on Windows monotonic ticks every 15.6 ms.
        self.wall_start: float = time.perf_counter()
        self.last: float = self.wall_start
        self.adjusted: float = 0.0
        # The highest load average it read: whether the run was ever slowed (`loaded`).
        self.peak: float = 0.0

    def now(self) -> float:
        with self.lock:
            t = time.perf_counter()
            load = system.loadavg()[0]
            self.peak = max(self.peak, load)
            self.adjusted += (t - self.last) / (stretch(load, self.cpus) * self.speed)
            self.last = t
            return self.adjusted

    def wall(self) -> float:
        return time.perf_counter() - self.wall_start

    def loaded(self) -> bool:
        """Whether the load average went over the cores while it ran."""
        return self.peak > self.cpus


class WallClock(Clock):
    """Wall seconds, for a deadline that someone else's wall clock enforces (bisect.py)."""

    def now(self) -> float:
        return self.wall()


def took(wall: float, adjusted: float) -> str:
    """A duration: the adjusted seconds, with the wall seconds beside them when the two differ
    by more than a tenth."""
    if adjusted * 0.9 <= wall <= adjusted * 1.1:
        return f"{wall:.1f} s"
    return f"{adjusted:.1f} s ({wall:.0f} s on the clock)"
