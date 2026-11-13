"""The machine's CPU tokens, shared by every heavy command `by` starts on it.

With 5 to 10 worktrees active the load average sat near 30 and reached 83 on 18 cores, while the
cores in use averaged 8 to 10: each cargo build, test run and check chose its parallelism from the
core count as if it had the machine to itself, so work arrived in bursts that slowed everything
at once, including the checks that gate a push. Now a heavy command first takes some of a fixed
number of tokens, one per core, and runs with as many threads as it got (`Lease.env`). A command
that finds none free waits, which costs nothing.

A token is a file in the pool directory, `<cache>/cpu-tokens/<n>`, holding the pid and class of
the process that took it. The pool's lock file guards every change. A token whose holder has
exited, such as a check killed at its deadline, is taken back by the next process that looks.

Each command has a class, highest first: `push` (the fast check of `enact push`), `agent` (a
command a session runs by hand, such as `just test-rust`) and `ci` (slow CI). A class may take a
token only while the tokens held by it and every lower class stay under its ceiling (`ceiling`),
so a push always finds a quarter of the cores that only it can take, and slow CI never holds more
than half. A class also takes nothing while a higher class waits.

Long background work (tournaments, the nightly, `just blunder-hunt`) takes no tokens. It runs at
utility QoS (the justfile's `low`, `system.utility`), under which macOS gives every core to
normal-priority work first, thread by thread, which no token count can match. A lease is fixed
when it starts, so holding it to a few tokens would leave the cores idle for hours whenever the
machine is quiet.

Cargo can share a pool through the GNU make jobserver protocol (the `jobserver` crate cargo uses
reads `--jobserver-auth=fifo:PATH` from `CARGO_MAKEFLAGS` or `MAKEFLAGS`), but a token is a byte
read from the FIFO, so a client killed while it holds one loses it for good, and the checks kill
steps at their deadlines. Nor can it order its waiters by priority. So the pool hands out whole
leases instead, and passes their size to cargo (`CARGO_BUILD_JOBS`), nextest
(`NEXTEST_TEST_THREADS`), the test harness (`RUST_TEST_THREADS`) and Beryl's own search commands
(`BERYL_CPU_TOKENS`).

Browser pages keep their own slots (ui/scripts/slots.mjs). The step that opens them, such as
`ui-test`, holds tokens for the build and the Node side, and a page holds a slot for a few
seconds at a time from Node, which would need the same protocol written again there.
"""

from __future__ import annotations

import argparse
import os
import random
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from . import paths, system

CLASSES: tuple[str, ...] = ("push", "agent", "ci")

# Set in the environment of a command that holds a lease, to its size: a command it starts,
# such as `by test-rust` under a check step, runs inside that lease instead of waiting for
# tokens its parent holds.
LEASE_ENV = "BERYL_CPU_TOKENS"

# `0` turns the pool off: every lease is granted at once, with every core.
POOL_ENV = "BERYL_CPU_POOL"

# A check's steps that run many threads (`lane:step`), which each want `largest` tokens. Every
# other step is a script that keeps one core busy at most and takes none.
HEAVY_STEPS: frozenset[str] = frozenset(
    {
        "rust:clippy",
        "rust:tests",
        "app:smoke",
        "shots:ui-shots",
        "ui-test:ui-test",
        "oracle:oracle",
        "mobile:mobile-check",
        "script:script-check",
        "motion:motion",
        "counts:count-gate",
    }
)


# The threads of one process (a check's lanes) take leases at once. The file lock orders
# processes, and this orders the threads that share a pid, whichever `Pool` they use.
_THREADS = threading.Lock()


def pool_size() -> int:
    return os.cpu_count() or 1


def ceiling(klass: str, total: int) -> int:
    """The most tokens `klass` and every class below it may hold together."""
    return {
        "push": total,
        "agent": max(1, total - total // 4),
        "ci": max(1, total // 2),
    }[klass]


def largest(total: int) -> int:
    """The most tokens one lease takes: two thirds of the pool, so the heavy steps of one check
    that start together, such as the Rust tests and the browser checks, split it instead of the
    first taking every token and the second waiting for it to finish."""
    return max(1, -(-total * 2 // 3))


def step_tokens(lane: str, step: str) -> int:
    """The tokens a check's step wants: zero (it takes none) unless it is heavy."""
    return largest(pool_size()) if f"{lane}:{step}" in HEAVY_STEPS else 0


@dataclass
class Holder:
    pid: int
    klass: str


def _read(path: Path) -> Holder | None:
    try:
        pid, klass = path.read_text().split()
        return Holder(int(pid), klass) if klass in CLASSES else None
    except (OSError, ValueError):
        return None


@dataclass
class Lease:
    """Tokens one process took, `size` of them (at least one), released on exit from the
    `with` block. A lease inside another (`inherited`) owns no files."""

    size: int
    files: list[Path]
    waited: float = 0.0
    inherited: bool = False

    def env(self) -> dict[str, str]:
        """Environment variables that hold a command and its children to the lease."""
        if not self.size:
            return {}
        n = str(self.size)
        return {
            LEASE_ENV: n,
            "CARGO_BUILD_JOBS": n,
            "NEXTEST_TEST_THREADS": n,
            "RUST_TEST_THREADS": n,
        }

    def release(self) -> None:
        for f in self.files:
            try:
                f.unlink()
            except FileNotFoundError:
                pass  # Taken back as stale already.
        self.files = []

    def __enter__(self) -> Lease:
        return self

    def __exit__(self, *_exc: object) -> None:
        self.release()


class Pool:
    """The token files in `root`, `size` of them."""

    def __init__(self, root: Path | None = None, size: int | None = None) -> None:
        self.root: Path = root or paths.cache_root() / "cpu-tokens"
        self.size: int = size or pool_size()
        self.root.mkdir(parents=True, exist_ok=True)

    def holders(self) -> dict[Path, Holder]:
        """The live tokens, taking back those whose holder exited. Call under the lock."""
        held: dict[Path, Holder] = {}
        for i in range(self.size):
            path = self.root / str(i)
            if not path.exists():
                continue
            holder = _read(path)
            if holder is None or not system.alive(holder.pid):
                path.unlink(missing_ok=True)
                continue
            held[path] = holder
        return held

    def waiters(self) -> list[tuple[Path, Holder]]:
        """The live waiters' files, removing those whose process exited. Call under the
        lock."""
        live: list[tuple[Path, Holder]] = []
        for path in self.root.glob("wait-*"):
            holder = _read(path)
            if holder is None or not system.alive(holder.pid):
                path.unlink(missing_ok=True)
                continue
            live.append((path, holder))
        return live

    def grant(self, klass: str, want: int, me: Path) -> list[Path]:
        """The token files taken for a lease of up to `want` tokens; none when the class may
        take none now. Call under the lock."""
        rank = CLASSES.index(klass)
        held = self.holders()
        below = sum(1 for h in held.values() if CLASSES.index(h.klass) >= rank)
        room = min(self.size - len(held), ceiling(klass, self.size) - below)
        rivals = 0
        for path, w in self.waiters():
            if path == me:
                continue
            other = CLASSES.index(w.klass)
            if other < rank:
                return []
            if other == rank:
                rivals += 1
        # Waiters of the same class share what is free rather than the first taking it all.
        share = max(1, room // (rivals + 1)) if rivals else room
        n = min(want, room, share)
        if n < 1:
            return []
        free = [self.root / str(i) for i in range(self.size)]
        free = [p for p in free if p not in held][:n]
        mine = f"{os.getpid()} {klass}"
        for path in free:
            path.write_text(mine)
        return free

    def acquire(
        self,
        klass: str,
        want: int,
        give_up: Callable[[], bool] = lambda: False,
    ) -> Lease | None:
        """A lease of between one and `want` tokens of `klass`, waiting while none is free;
        None if `give_up` turns true first. Inside another lease (`LEASE_ENV`), that lease.
        """
        inherited = os.environ.get(LEASE_ENV, "")
        if inherited.isdigit() and int(inherited) > 0:
            return Lease(int(inherited), [], inherited=True)
        if os.environ.get(POOL_ENV) == "0":
            return Lease(max(1, want), [])
        want = max(1, min(want, self.size))
        # One file per waiting lease: the threads of one process wait apart.
        me = self.root / f"wait-{os.getpid()}-{threading.get_ident()}-{klass}"
        started = time.monotonic()
        try:
            while True:
                with _THREADS, system.file_lock(self.root / "lock"):
                    files = self.grant(klass, want, me)
                    if files:
                        return Lease(len(files), files, time.monotonic() - started)
                    if not me.exists():
                        me.write_text(f"{os.getpid()} {klass}")
                if give_up():
                    return None
                time.sleep(0.2 + random.random() * 0.2)
        finally:
            me.unlink(missing_ok=True)


def acquire(
    klass: str, want: int, give_up: Callable[[], bool] = lambda: False
) -> Lease | None:
    """A lease from the machine's pool (`Pool.acquire`)."""
    return Pool().acquire(klass, want, give_up)


def lease(klass: str, want: int, give_up: Callable[[], bool]) -> Lease | None:
    """A check step's lease (`acquire`): with `want` zero, an empty one that holds the step to
    nothing, at once."""
    return acquire(klass, want, give_up) if want else Lease(0, [])


def script_class(script: str) -> str:
    """The class of a check run's steps: `check` is the fast check of a push and `ci` is slow
    CI; any other run is a session's own."""
    return {"check": "push", "ci": "ci"}.get(script, "agent")


def status(_args: argparse.Namespace) -> int:
    pool = Pool()
    with system.file_lock(pool.root / "lock"):
        held = pool.holders()
        waiting = pool.waiters()
    print(f"{len(held)} of {pool.size} tokens held")
    for klass in CLASSES:
        pids: dict[int, int] = {}
        for h in held.values():
            if h.klass == klass:
                pids[h.pid] = pids.get(h.pid, 0) + 1
        queued = sum(1 for _, w in waiting if w.klass == klass)
        if pids or queued:
            leases = ", ".join(f"pid {p}: {n}" for p, n in sorted(pids.items()))
            print(f"  {klass}: {leases or 'none held'}; {queued} waiting")
    return 0


def run(args: argparse.Namespace) -> int:
    """Runs a command inside a lease of `--class` tokens, at most `--cores`."""
    command = args.extra[1:] if args.extra[:1] == ["--"] else args.extra
    if not command:
        print("tokens run: no command", file=sys.stderr)
        return 2
    lease = acquire(args.klass, args.cores or largest(pool_size()))
    assert lease is not None
    with lease:
        env = {**os.environ, **lease.env()}
        return subprocess.run(
            [system.which(command[0]), *command[1:]], env=env, check=False
        ).returncode


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("tokens", help="the machine's CPU tokens: who holds and waits")
    p.set_defaults(func=status)
    r = sub.add_parser(
        "tokens-run",
        help="run a command inside a lease of CPU tokens (--class C, --cores N, -- COMMAND...)",
    )
    r.add_argument("--class", dest="klass", choices=CLASSES, default="agent")
    r.add_argument("--cores", type=int, default=0, help="the most tokens to take")
    r.set_defaults(func=run, passthrough=True)
