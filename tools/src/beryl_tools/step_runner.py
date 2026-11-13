"""The steps of a check run (checks.py), each started in its own session and killed at the
run's deadline, after waiting for the CPU tokens it wants (tokens.py)."""

from __future__ import annotations

import subprocess
import tempfile
import threading
from pathlib import Path
from typing import Any

from . import system, tokens
from .clock import Clock


class Steps:
    """A run's steps, each started in its own session, so that once the run's deadline passes,
    or the run is interrupted, a step can be killed with everything it started."""

    def __init__(
        self,
        deadline: float,
        clock: Clock | None = None,
        wall_limit: float | None = None,
        klass: str = "agent",
    ) -> None:
        self.deadline = deadline
        self.klass = klass
        self.clock: Clock = clock or Clock()
        # Wall seconds (`Clock.wall`) before someone else's wall clock kills the run, such as
        # Enact's `slow_timeout`: a loaded machine stretches the deadline past it,
        # and a run killed from outside prints nothing, so a step still running then is
        # killed here instead and named as timed out.
        self.wall_limit = wall_limit
        self.stopped = threading.Event()
        # Reentrant: `over` reads the lane's waits under it, and `run` calls `over` under it.
        self.lock = threading.RLock()
        # Adjusted and wall seconds each lane (the thread that runs it) spent waiting for CPU
        # tokens.
        # They extend that lane's deadline: a wait is no hang and no slow step, and counting
        # it once left a lane's tests eight seconds after its clippy step waited 290 s.
        self.waits: dict[int, tuple[float, float]] = {}
        # The lanes with heavy steps (`tokens.step_tokens`) still running, which share the
        # pool between them (`play` and `run_lane` keep it).
        self.heavy_lanes = 0
        # Popen's type depends on its arguments, which pyre reads differently per platform.
        self.running: set[subprocess.Popen[Any]] = set()

    def waited(self) -> tuple[float, float]:
        """The adjusted and wall seconds the calling thread's lane has waited for tokens so
        far."""
        with self.lock:
            return self.waits.get(threading.get_ident(), (0.0, 0.0))

    def left(self) -> float:
        """Seconds before the deadline, which the calling lane's waits for tokens push back,
        or the wall limit, whichever comes first."""
        left = self.deadline + self.waited()[0] - self.clock.now()
        if self.wall_limit is not None:
            left = min(left, self.wall_limit - self.clock.wall())
        return left

    def over(self) -> bool:
        """Whether the run was stopped or its time is up, so no step may start."""
        return self.stopped.is_set() or self.left() <= 0

    def cut(self) -> bool:
        """Whether a step waiting for tokens must give up: the run was stopped or the wall
        limit passed. The deadline does not apply while it waits."""
        return self.stopped.is_set() or self.walled()

    def tokens(self, lane: str, step: str) -> int:
        """The tokens a lane's step wants, its share of the pool (`tokens.step_tokens`)."""
        with self.lock:
            lanes = self.heavy_lanes
        return tokens.step_tokens(lane, step, lanes)

    def walled(self) -> bool:
        """Whether the wall limit has passed, so a step killed now was cut off by it."""
        return self.wall_limit is not None and self.wall_limit - self.clock.wall() <= 0

    def limit(self) -> str:
        """What a step that timed out ran into, for its line in the log."""
        if self.wall_limit is not None and self.walled():
            return f"the wall limit of {self.wall_limit:.0f} s before Enact's kill"
        return f"deadline {self.deadline:.0f} s"

    def run(
        self, cmd: list[str], cwd: Path, env: dict[str, str], cores: int = 0
    ) -> tuple[int | None, str]:
        """The step's exit code, None when the deadline killed it, and its output. The output
        goes to a file, not a pipe, so a killed step's straggler cannot hold up reading it.
        It first waits for up to `cores` CPU tokens (tokens.py); the wait moves the lane's
        deadline back by as long (`waited`).
        """
        began, wall = self.clock.now(), self.clock.wall()
        lease = tokens.lease(self.klass, cores, self.cut)
        if cores:
            with self.lock:
                me = threading.get_ident()
                adjusted, walled = self.waits.get(me, (0.0, 0.0))
                self.waits[me] = (
                    adjusted + self.clock.now() - began,
                    walled + self.clock.wall() - wall,
                )
        if lease is None:
            return None, ""
        with tempfile.TemporaryFile("w+", errors="replace") as out, lease:
            env = {**env, **lease.env()}
            with self.lock:
                if self.over():
                    return None, ""
                proc = subprocess.Popen(
                    cmd,
                    cwd=cwd,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    stdout=out,
                    stderr=subprocess.STDOUT,
                    **system.own_session(),
                )
                self.running.add(proc)
            code: int | None = None
            try:
                while (left := self.left()) > 0:
                    try:
                        code = proc.wait(timeout=min(0.5, left))
                        break
                    except subprocess.TimeoutExpired:
                        continue
                else:
                    system.kill_tree(proc.pid)
                    proc.wait()
            finally:
                with self.lock:
                    self.running.discard(proc)
            out.seek(0)
            return code, out.read()

    def stop(self) -> None:
        """Kills every running step and starts no more."""
        with self.lock:
            self.stopped.set()
            for proc in self.running:
                system.kill_tree(proc.pid)
