"""The steps of a check run (checks.py), each started in its own session and killed when it runs
past its hang limit (hang.py), after waiting for the CPU tokens it wants (tokens.py)."""

from __future__ import annotations

import math
import os
import subprocess
import time
import tempfile
import threading
from pathlib import Path
from typing import Any

from . import system, tokens
from .clock import Clock


def reap(
    proc: subprocess.Popen[Any], timeout: float | None
) -> tuple[int | None, float]:
    """Waits up to `timeout` seconds (forever for None) for `proc` to exit: its exit code, None
    if it is still running, and the CPU seconds (user plus system) it and every process under it
    used. The kernel adds a waited-for process's own waited-for descendants to its usage, so
    cargo's rustc processes and a test's helpers count, and a process that was orphaned (a
    daemon, or a browser page launchd started) does not. Windows has no `wait4`: the code is
    `proc.wait`'s and the CPU time is 0."""
    if system.WINDOWS:
        try:
            return proc.wait(timeout), 0.0
        except subprocess.TimeoutExpired:
            return None, 0.0
    deadline = None if timeout is None else time.monotonic() + timeout
    while True:
        # Popen would discard the usage `wait4` returns, so wait here and tell it the result.
        pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
        if pid:
            proc.returncode = os.waitstatus_to_exitcode(status)
            return proc.returncode, usage.ru_utime + usage.ru_stime
        if deadline is not None and time.monotonic() >= deadline:
            return None, 0.0
        time.sleep(0.02)


class Steps:
    """A run's steps, each started in its own session, so that a step past its hang limit, or
    every step once the run is interrupted, can be killed with everything it started."""

    def __init__(
        self,
        clock: Clock | None = None,
        wall_limit: float | None = None,
        klass: str = "agent",
    ) -> None:
        self.klass = klass
        self.clock: Clock = clock or Clock()
        # Wall seconds (`Clock.wall`) before someone else's wall clock kills the run, such as
        # Enact's `slow_timeout`: a run killed from outside prints nothing, so a step still
        # running then is killed here instead and named as cut off.
        self.wall_limit = wall_limit
        self.stopped = threading.Event()
        # Reentrant: `run` calls `over` under it.
        self.lock = threading.RLock()
        # Adjusted and wall seconds each lane (the thread that runs it) spent waiting for CPU
        # tokens, which the lane's and its steps' times leave out (checks.lane_steps).
        self.waits: dict[int, tuple[float, float]] = {}
        # The CPU seconds the steps each lane ran have used so far (`reap`), the same way.
        self.cpu: dict[int, float] = {}
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

    def cpu_used(self) -> float:
        """The CPU seconds of the steps the calling thread's lane has run so far."""
        with self.lock:
            return self.cpu.get(threading.get_ident(), 0.0)

    def wall_left(self) -> float:
        """Wall seconds before the wall limit, infinite without one."""
        if self.wall_limit is None:
            return math.inf
        return self.wall_limit - self.clock.wall()

    def over(self) -> bool:
        """Whether the run was stopped or its wall limit passed, so no step may start."""
        return self.stopped.is_set() or self.walled()

    def cut(self) -> bool:
        """Whether a step waiting for tokens must give up: the run was stopped or the wall
        limit passed. A hang limit does not apply while it waits."""
        return self.over()

    def tokens(self, lane: str, step: str) -> int:
        """The tokens a lane's step wants, its share of the pool (`tokens.step_tokens`)."""
        with self.lock:
            lanes = self.heavy_lanes
        return tokens.step_tokens(lane, step, lanes)

    def walled(self) -> bool:
        """Whether the wall limit has passed, so a step killed now was cut off by it."""
        return self.wall_left() <= 0

    def run(
        self,
        cmd: list[str],
        cwd: Path,
        env: dict[str, str],
        cores: int = 0,
        limit: float | None = None,
    ) -> tuple[int | None, str]:
        """The step's exit code, None when it was killed, and its output. The output goes to a
        file, not a pipe, so a killed step's straggler cannot hold up reading it. It first waits
        for up to `cores` CPU tokens (tokens.py), and the wait is recorded in `waits`. Then it
        runs until it exits, the run is stopped, the wall limit passes, or it has run `limit`
        adjusted seconds, its hang limit (hang.py); the output of a step killed at its hang
        limit ends with the processes it left running.
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
            started = self.clock.now()
            code: int | None = None
            hung = ""
            try:
                while True:
                    mine = (
                        math.inf
                        if limit is None
                        else limit - (self.clock.now() - started)
                    )
                    left = min(mine, self.wall_left())
                    if left <= 0:
                        if mine <= 0:
                            hung = system.process_tree(proc.pid)
                        system.kill_tree(proc.pid)
                        self.charge(reap(proc, None)[1])
                        break
                    code, used = reap(proc, min(0.5, left))
                    if code is not None:
                        self.charge(used)
                        break
            finally:
                with self.lock:
                    self.running.discard(proc)
            out.seek(0)
            output = out.read()
            if hung:
                output = (
                    f"{output.rstrip()}\n--- processes when it was killed ---\n{hung}\n"
                )
            return code, output

    def charge(self, seconds: float) -> None:
        """Adds a finished step's CPU seconds to the calling thread's lane."""
        with self.lock:
            me = threading.get_ident()
            self.cpu[me] = self.cpu.get(me, 0.0) + seconds

    def stop(self) -> None:
        """Kills every running step and starts no more."""
        with self.lock:
            self.stopped.set()
            for proc in self.running:
                system.kill_tree(proc.pid)
