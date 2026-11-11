"""What differs between macOS and Windows: executables' names, Node's command shims, the load
average, starting a process tree that can be killed whole, and running work at low priority.
"""

from __future__ import annotations

import argparse
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

from . import paths

WINDOWS: bool = sys.platform == "win32"


def exe(path: Path) -> Path:
    """`path` as an executable's name: with `.exe` on Windows."""
    return path.with_name(path.name + ".exe") if WINDOWS else path


def npm_bin(name: str) -> str:
    """A command node_modules/.bin installed in ui/: a `.cmd` shim on Windows."""
    return str(
        paths.UI / "node_modules" / ".bin" / (name + (".cmd" if WINDOWS else ""))
    )


def which(name: str) -> str:
    """The command `name` as subprocess must be given it: Windows runs no `.cmd` shim, such
    as npm's or npx's, by its bare name."""
    return (shutil.which(name) or name) if WINDOWS else name


class _CpuSampler:
    """Windows has no load average: it stands in the cores in use, from the system's idle and
    busy times since the last sample, as a load of that many threads. It never exceeds the
    core count, as a run queue would."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.last: tuple[int, int] | None = None
        self.at = 0.0
        self.load = 0.0

    @staticmethod
    def times() -> tuple[int, int]:
        import ctypes
        from ctypes import wintypes

        idle, kernel, user = (
            wintypes.FILETIME(),
            wintypes.FILETIME(),
            wintypes.FILETIME(),
        )
        ctypes.windll.kernel32.GetSystemTimes(  # pyre-ignore[16]: Windows only
            ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)
        )

        def ticks(t: Any) -> int:
            return (t.dwHighDateTime << 32) | t.dwLowDateTime

        # Kernel time includes idle time.
        return ticks(idle), ticks(kernel) + ticks(user)

    def sample(self) -> float:
        with self.lock:
            if self.last is None:
                self.last = self.times()
                time.sleep(0.1)
            elif time.monotonic() - self.at < 0.1:
                return self.load
            idle, total = self.times()
            last = self.last
            assert last is not None
            last_idle, last_total = last
            elapsed = total - last_total
            if elapsed > 0:
                busy = 1.0 - (idle - last_idle) / elapsed
                self.load = max(0.0, busy) * (os.cpu_count() or 1)
            self.last, self.at = (idle, total), time.monotonic()
            return self.load


_SAMPLER = _CpuSampler()


def loadavg() -> tuple[float, float, float]:
    """The 1, 5 and 15 minute load averages; on Windows, the cores in use now, thrice."""
    if WINDOWS:
        load = _SAMPLER.sample()
        return load, load, load
    return os.getloadavg()


def own_session() -> dict[str, Any]:
    """Popen arguments that start a process apart from this one's Ctrl-C, as the leader of a
    tree [`kill_tree`] can end whole."""
    if WINDOWS:
        # pyre-ignore[16]: Windows only
        return {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    return {"start_new_session": True}


def kill_tree(pid: int) -> None:
    """Kills process `pid` and every process under it, including any that left its process
    group (cargo-nextest gives each test its own)."""
    if WINDOWS:
        subprocess.run(
            ["taskkill", "/F", "/T", "/PID", str(pid)],
            capture_output=True,
            check=False,
        )
        return
    table = subprocess.run(
        ["ps", "-A", "-o", "pid=,ppid="], capture_output=True, text=True, check=False
    ).stdout
    children: dict[int, list[int]] = {}
    for row in table.splitlines():
        child, parent = map(int, row.split())
        children.setdefault(parent, []).append(child)
    tree: list[int] = []
    frontier = [pid]
    while frontier:
        frontier = [c for p in frontier for c in children.get(p, [])]
        tree += frontier
    sigkill = getattr(signal, "SIGKILL")
    try:
        os.killpg(pid, sigkill)
    except (ProcessLookupError, PermissionError):
        pass
    for p in tree:
        try:
            os.kill(p, sigkill)
        except (ProcessLookupError, PermissionError):
            pass


def terminate(proc: subprocess.Popen[bytes]) -> None:
    """Ends `proc`. On Windows with everything under it: there a `.cmd` shim's process is
    cmd.exe, and ending only it leaves the program it started running."""
    if WINDOWS:
        kill_tree(proc.pid)
    else:
        proc.terminate()


class LockHeld(Exception):
    """Another process held a lock past the wait for it."""


@contextmanager
def file_lock(path: Path, timeout: float | None = None) -> Iterator[None]:
    """Holds an exclusive lock on the file at `path` for the duration, waiting for it: forever,
    or at most `timeout` seconds, then raising LockHeld."""
    deadline = None if timeout is None else time.monotonic() + timeout
    with open(path, "w") as handle:
        if WINDOWS:
            import msvcrt

            # LK_NBLCK tries once; LK_LOCK gives up after ten tries a second apart.
            while True:
                try:
                    # pyre-ignore[16]: Windows only
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    if deadline is not None and time.monotonic() >= deadline:
                        raise LockHeld(f"{path} held for {timeout:g} s")
                    time.sleep(0.2)
            try:
                yield
            finally:
                handle.seek(0)
                # pyre-ignore[16]: Windows only
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            return
        import fcntl

        if deadline is None:
            fcntl.flock(handle, fcntl.LOCK_EX)
        else:
            while True:
                try:
                    fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise LockHeld(f"{path} held for {timeout:g} s")
                    time.sleep(0.2)
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def low(args: argparse.Namespace) -> int:
    """Runs a command below normal priority, which its children inherit, so other work gets
    the CPU first; with `--awake`, keeping the machine from sleeping meanwhile. Windows' stand-in
    for `taskpolicy -c utility` and `caffeinate -i`."""
    command = args.extra[1:] if args.extra[:1] == ["--"] else args.extra
    if not command:
        print("low: no command", file=sys.stderr)
        return 2
    if not WINDOWS:
        return subprocess.run(["nice", "-n", "10", *command], check=False).returncode
    import ctypes

    kernel32 = ctypes.windll.kernel32  # pyre-ignore[16]: Windows only
    es_continuous, es_system_required = 0x80000000, 0x00000001
    if args.awake:
        kernel32.SetThreadExecutionState(es_continuous | es_system_required)
    proc = subprocess.Popen(
        [which(command[0]), *command[1:]],
        # pyre-ignore[16]: Windows only
        creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS,
    )
    try:
        return proc.wait()
    except KeyboardInterrupt:
        # The console sent Ctrl-C to the child too: it answers it.
        return proc.wait()
    finally:
        if args.awake:
            kernel32.SetThreadExecutionState(es_continuous)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "low", help="run a command at low priority (-- COMMAND...), as Windows can"
    )
    p.add_argument("--awake", action="store_true", help="keep the machine awake")
    p.set_defaults(func=low, passthrough=True)
