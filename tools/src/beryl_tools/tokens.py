"""The machine's CPU tokens, shared by every heavy command `by` starts on it.

With 5 to 10 worktrees active the load average sat near 30 and reached 83 on 18 cores, while the
cores in use averaged 8 to 10: each cargo build, test run and check chose its parallelism from the
core count as if it had the machine to itself, so work arrived in bursts that slowed everything
at once, including the checks that gate a push. Now a heavy command first takes some of a fixed
number of tokens, one per core, and runs with as many threads as it got (`Lease.env`). A command
that finds none free waits, which costs nothing.

A token is a file in the pool directory, `<cache>/cpu-tokens/<n>`, holding the pid and class of
the process that took it. The pool's lock file guards every change. A token whose holder has
exited, such as a check step killed as hung, is taken back by the next process that looks.

Each command has a class, highest first: `queue` (the fast check Enact's merge queue runs on a
batch of pushes, in its tree `.worktrees/.merge-queue`), `push` (the fast check `enact push`
runs in the branch's own worktree first), `agent` (a command a session runs by hand, such as
`just test-rust`), `ci` (slow CI) and `sweep` (the browser and Beryl 3D sweeps and their
bisections, `SWEEP_SCRIPTS`). A class may take a token only while the tokens held by it
and every lower class stay under its ceiling (`ceiling`), so a push always finds a quarter of the
cores that agents and slow CI cannot take, and the queue an eighth that no push can. A class also
takes nothing while a higher class waits.

The queue's check comes first because every push on the machine waits for it, one batch at a
time, while the pushes' own checks run side by side. With several sessions pushing at once, each
push's check would otherwise take an equal share and leave the queue's check a fraction of the
cores. Slow CI once had a ceiling of half the pool as well, and its runs
timed out with half the cores idle: ten heavy steps shared nine tokens whether or not anything
else wanted the rest. Now it may take any token an agent may, and the waiting rule alone puts
pushes and agents first.

Cargo commands outside a check, whether `by` starts them or a session types them, take tokens
through a shim that stands in for cargo on PATH (cargo_pool.py), which also lets a build grow
while it runs.

The sweeps once ran at utility QoS with no tokens, as the nightly does. At the machine's usual
load of 40 to 70 the utility class got almost no CPU while agents' checks ran: on 2026-11-15 the
browser sweep took 6,317 s and 12,364 s, longer than its 3-hour interval, and Beryl 3D's sweep
spent 1,768 s on a format check that used 28 CPU-seconds. Slow CI, at normal priority with
tokens, used about as many CPU-seconds as wall seconds on the same days. So the sweeps now run
at normal priority as the lowest class, which together holds at most a quarter of the pool
(`ceiling`): a bounded share they always get, instead of what is left over.

Other long background work (tournaments, the nightly, `just blunder-hunt`) takes no tokens. It runs at
utility QoS (the justfile's `low`, `system.utility`), under which macOS gives every core to
normal-priority work first, thread by thread, which no token count can match. A lease is fixed
when it starts, so holding it to a few tokens would leave the cores idle for hours whenever the
machine is quiet.

Cargo can share a pool through the GNU make jobserver protocol (the `jobserver` crate cargo uses
reads `--jobserver-auth=fifo:PATH` from `CARGO_MAKEFLAGS` or `MAKEFLAGS`), but a token is a byte
read from the FIFO, so a client killed while it holds one loses it for good, and the checks kill
steps that hang. Nor can it order its waiters by priority. So the pool hands out whole
leases instead, and passes their size to cargo (`CARGO_BUILD_JOBS`), nextest
(`NEXTEST_TEST_THREADS`), the test harness (`RUST_TEST_THREADS`) and Beryl's own search commands
(`BERYL_CPU_TOKENS`).

A lease is fixed when its step starts, so a step that starts while other heavy lanes run gets a
small share and keeps it after they finish. On 2026-11-14 the fast check's Rust tests step started
beside the two browser lanes, took 6 of 18 tokens, and ran nextest 6 tests wide for 44 s after
those lanes ended about 20 s in, with 12 cores idle. A command inside a lease can take the tokens
free at a later point without waiting (`top_up`), as the tests step does once its build is done.

A lease's size is also fixed from the other side: a long build of a lower class keeps the tokens
it took while a push that starts later gets what is left. Between 10:00 UTC 2026-11-15 and the
same time the next day, 933 of 1,943 push leases got a single token, though most wanted 12; at
those moments agents held about 11 of the 18 tokens, mostly through `cargo build` (54 of the
agents' 112 token-hours). The fast check then hit its 180 s limit in 296 of 544 runs. So a lease
whose command can shrink marks its tokens after the first as `elastic` in their files
(`Holder.elastic`). Cargo builds started through the shim do this (cargo_pool.py), and hand such
tokens back while a higher class waits (`higher_waiting`, `give_back`). A lease that would get
fewer tokens than it wants waits up to `RECLAIM_SECONDS` while lower classes hold elastic tokens
(`reclaimable`), as a waiter, which stops them taking more and makes them give theirs back. Only
then does it take what is free.

When a lease that owns tokens is released, the pool appends one line to `leases.jsonl` beside the
tokens ([`LEASE_LOG`]): when, the class, the worktree, the command, the tokens it got (`cores`)
and wanted, and the seconds it waited and held them. The efficiency dashboard reads it for the
time commands queued for cores (efficiency/, "CPU accounting" in docs/efficiency-dashboard.md).
The line is written under the pool's lock, which the releasing process takes anyway to return
its tokens, and a failed write is reported without failing the command. An inherited lease and
a top-up (extra tokens for a lease already logged, taken without waiting) write nothing.

Browser pages keep their own slots (ui/scripts/slots.mjs). The step that opens them, such as
`ui-test`, holds tokens for the build and the Node side, and a page holds a slot for a few
seconds at a time from Node, which would need the same protocol written again there.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import paths, system

CLASSES: tuple[str, ...] = ("queue", "push", "agent", "ci", "sweep")

# The run-log scripts of the sweeps (sweep.py and beryl3d/sweep.py), whose steps and bisections
# take tokens as `sweep`.
SWEEP_SCRIPTS: frozenset[str] = frozenset({"sweep", "b3d-sweep"})

# The name of the tree in which Enact's merge queue checks a batch of pushes (`QUEUE_TREE` in
# enact.py), whose fast check takes tokens as `queue`.
QUEUE_TREE = ".merge-queue"

# Set in the environment of a command that holds a lease, to its size: a command it starts,
# such as `by test-rust` under a check step, runs inside that lease instead of waiting for
# tokens its parent holds.
LEASE_ENV = "BERYL_CPU_TOKENS"

# Set beside `LEASE_ENV` to the lease's class, so that a command inside it can take more tokens
# of the same class (`top_up`).
CLASS_ENV = "BERYL_CPU_CLASS"

# `0` turns the pool off: every lease is granted at once, with every core.
POOL_ENV = "BERYL_CPU_POOL"

# The file in the pool's directory that gets one line per released lease (see the module
# comment), and how many days of lines it keeps, as many as the CPU sampler's rows (cpu.py).
LEASE_LOG = "leases.jsonl"
LEASE_LOG_DAYS = 30

# The longest a lease that would come up short waits for lower classes to give back their elastic
# tokens (see the module comment). A cargo build checks every `cargo_pool.GROW_SECONDS` and can
# give back a job slot only once a compiler job ends, so this allows a few of each.
RECLAIM_SECONDS = 10.0

# The word after the class in a token's file whose lease can give it back while a higher class
# waits.
ELASTIC = "elastic"

# A check's steps that run many threads (`lane:step`), which each want a share of the pool
# (`step_tokens`). Every other step is a script that keeps one core busy at most and takes none.
HEAVY_STEPS: frozenset[str] = frozenset(
    {
        "rust:clippy",
        "rust:tests",
        "app:smoke",
        "shots:ui-shots",
        "ui-test:ui-test",
        "ui-smoke:ui-test",
        "oracle:oracle",
        "mobile:mobile-check",
        "script:script-check",
        "motion:motion",
        "counts:count-gate",
        "counts:count-gate-families",
    }
)


# The threads of one process (a check's lanes) take leases at once. The file lock orders
# processes, and this orders the threads that share a pid, whichever `Pool` they use.
_THREADS = threading.Lock()


def pool_size() -> int:
    return os.cpu_count() or 1


def ceiling(klass: str, total: int) -> int:
    """The most tokens `klass` and every class below it may hold together: every token for the
    merge queue's check, all but an eighth for pushes, which leaves those to the queue, three
    quarters for agents and slow CI, which leaves the rest to pushes and the queue, and a quarter
    for the sweeps, so that an hour-long sweep step never holds cores a check will want.
    """
    if klass == "queue":
        return total
    if klass == "push":
        return max(1, total - total // 8)
    if klass == "sweep":
        return max(1, total // 4)
    return max(1, total - total // 4)


def largest(total: int) -> int:
    """The most tokens one lease takes: two thirds of the pool, so the heavy steps of one check
    that start together, such as the Rust tests and the browser checks, split it instead of the
    first taking every token and the second waiting for it to finish."""
    return max(1, -(-total * 2 // 3))


def step_tokens(lane: str, step: str, lanes: int = 1, total: int = 0) -> int:
    """The tokens a check's step wants: zero (it takes none) unless it is heavy, and otherwise
    an even share of the `total` tokens (the pool's size by default) among the `lanes` with
    heavy steps still running, at most `largest`. Slow CI starts about ten heavy lanes at once:
    if each wanted `largest`, the first would take most of the pool and the rest one token
    each, for the length of their steps."""
    if f"{lane}:{step}" not in HEAVY_STEPS:
        return 0
    total = total or pool_size()
    return max(1, min(largest(total), -(-total // max(1, lanes))))


@dataclass
class Holder:
    pid: int
    klass: str
    # Whether its lease gives this token back while a higher class waits.
    elastic: bool = False


def _read(path: Path) -> Holder | None:
    try:
        pid, klass, *rest = path.read_text().split()
        if klass not in CLASSES or rest not in ([], [ELASTIC]):
            return None
        return Holder(int(pid), klass, bool(rest))
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
    # The class it was taken for: empty for an inherited lease or one granted with the pool off.
    klass: str = ""
    # What the lease's line in the pool's log names; a lease with no `log` writes none.
    command: str = ""
    wanted: int = 0
    granted: float = 0.0
    log: Path | None = None

    def env(self) -> dict[str, str]:
        """Environment variables that hold a command and its children to the lease."""
        if not self.size:
            return {}
        n = str(self.size)
        klass = {CLASS_ENV: self.klass} if self.klass else {}
        return {
            **klass,
            LEASE_ENV: n,
            "CARGO_BUILD_JOBS": n,
            "NEXTEST_TEST_THREADS": n,
            "RUST_TEST_THREADS": n,
        }

    def release(self) -> None:
        log = self.log
        if not self.files or log is None:
            self._return_files()
            return
        with _THREADS, system.file_lock(log.parent / "lock"):
            self._return_files()
            self._write_log(log)

    def _return_files(self) -> None:
        for f in self.files:
            try:
                f.unlink()
            except FileNotFoundError:
                pass  # Taken back as stale already.
        self.files = []

    def _write_log(self, log: Path) -> None:
        """Appends this lease's line to `log`, first dropping the lines older than
        [`LEASE_LOG_DAYS`] when the oldest is; reports a failure instead of raising it.
        """
        now = time.time()
        line = {
            "time": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now)),
            "class": self.klass,
            "worktree": paths.REPO.name,
            "command": self.command,
            "cores": self.size,
            "wanted": self.wanted,
            "waited_s": round(self.waited, 1),
            "held_s": round(time.monotonic() - self.granted, 1),
        }
        try:
            drop_old_lines(log, now)
            with log.open("a") as handle:
                handle.write(json.dumps(line) + "\n")
        except (OSError, ValueError) as error:
            print(f"tokens: could not log the lease to {log}: {error}", file=sys.stderr)

    def __enter__(self) -> Lease:
        return self

    def __exit__(self, *_exc: object) -> None:
        self.release()


def drop_old_lines(log: Path, now: float) -> None:
    """Rewrites `log` without its lines from before the last [`LEASE_LOG_DAYS`] days, when its
    first line is that old. Lines are appended in time order, so reading the first is enough to
    know."""
    try:
        with log.open() as handle:
            first = handle.readline()
    except FileNotFoundError:
        return
    since = time.strftime(
        "%Y-%m-%dT%H:%M:%SZ", time.gmtime(now - LEASE_LOG_DAYS * 86400)
    )
    if not first.strip() or str(json.loads(first).get("time", "")) >= since:
        return
    kept = [
        line
        for line in log.read_text().splitlines(keepends=True)
        if line.strip() and str(json.loads(line).get("time", "")) >= since
    ]
    tmp = log.with_suffix(".tmp")
    tmp.write_text("".join(kept))
    tmp.replace(log)


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

    def grant(
        self,
        klass: str,
        want: int,
        me: Path,
        elastic_from: int | None = None,
        hold_out: bool = False,
    ) -> list[Path]:
        """The token files taken for a lease of up to `want` tokens; none when the class may
        take none now, or, with `hold_out`, when it would get fewer than `want` while lower
        classes hold elastic tokens they will give back. The files from index `elastic_from` on
        are marked elastic. Call under the lock."""
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
        if hold_out and n < want and reclaimable(held, rank):
            return []
        free = [self.root / str(i) for i in range(self.size)]
        free = [p for p in free if p not in held][:n]
        mine = f"{os.getpid()} {klass}"
        for i, path in enumerate(free):
            elastic = elastic_from is not None and i >= elastic_from
            path.write_text(f"{mine} {ELASTIC}" if elastic else mine)
        return free

    def higher_waiting(self, klass: str) -> bool:
        """Whether a lease of a class above `klass` waits for tokens. Takes the lock."""
        rank = CLASSES.index(klass)
        with _THREADS, system.file_lock(self.root / "lock"):
            return any(CLASSES.index(w.klass) < rank for _, w in self.waiters())

    def give_back(self, lease: Lease, n: int) -> int:
        """Returns up to `n` of `lease`'s elastic tokens to the pool at once, and how many."""
        with _THREADS, system.file_lock(self.root / "lock"):
            mine = [f for f in lease.files if _is_elastic(f)][:n]
            for f in mine:
                lease.files.remove(f)
                f.unlink(missing_ok=True)
        return len(mine)

    def acquire(
        self,
        klass: str,
        want: int,
        give_up: Callable[[], bool] = lambda: False,
        command: str = "",
        elastic_from: int | None = None,
    ) -> Lease | None:
        """A lease of between one and `want` tokens of `klass`, waiting while none is free, and
        for up to `RECLAIM_SECONDS` while lower classes would give back enough elastic tokens to
        make up `want`; None if `give_up` turns true first. Inside another lease (`LEASE_ENV`),
        that lease. Its release writes a line naming `command` to the pool's log (`LEASE_LOG`).
        Its tokens from index `elastic_from` on are elastic: the caller gives them back
        (`give_back`) while a higher class waits.
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
                    hold_out = time.monotonic() - started < RECLAIM_SECONDS
                    files = self.grant(klass, want, me, elastic_from, hold_out)
                    if files:
                        now = time.monotonic()
                        return Lease(
                            len(files),
                            files,
                            now - started,
                            klass=klass,
                            command=command,
                            wanted=want,
                            granted=now,
                            log=self.root / LEASE_LOG,
                        )
                    if not me.exists():
                        me.write_text(f"{os.getpid()} {klass}")
                if give_up():
                    return None
                time.sleep(0.2 + random.random() * 0.2)
        finally:
            me.unlink(missing_ok=True)

    def top_up(self, klass: str, held: int, want: int, elastic: bool = False) -> Lease:
        """The tokens of `klass` free now, without waiting, that bring a lease of `held` up to
        `want` in all: an empty lease when none is. The same rules as a waiting lease's apply
        (`grant`): a waiting higher class takes everything, and waiters of the same class
        share. With `elastic`, every token taken is elastic."""
        extra = min(want, self.size) - held
        if extra < 1:
            return Lease(0, [])
        # A waiter's file this call never writes, so `grant` counts no waiter as its own.
        me = self.root / f"wait-{os.getpid()}-{threading.get_ident()}-{klass}"
        with _THREADS, system.file_lock(self.root / "lock"):
            files = self.grant(klass, extra, me, 0 if elastic else None)
        return Lease(len(files), files, klass=klass)


def _is_elastic(path: Path) -> bool:
    holder = _read(path)
    return holder is not None and holder.elastic


def reclaimable(held: dict[Path, Holder], rank: int) -> int:
    """How many of the `held` tokens belong to classes ranked below `rank` and are elastic, so
    that their leases give them back while a lease of that rank waits."""
    return sum(1 for h in held.values() if h.elastic and CLASSES.index(h.klass) > rank)


def top_up(want: int) -> Lease:
    """Inside a lease another process holds (`LEASE_ENV` and `CLASS_ENV`), the tokens free now
    that bring it up to `want` in all (`Pool.top_up`); an empty lease outside one or with the
    pool off. The caller releases them when the work that uses them ends."""
    held = os.environ.get(LEASE_ENV, "")
    klass = os.environ.get(CLASS_ENV, "")
    if not held.isdigit() or klass not in CLASSES or os.environ.get(POOL_ENV) == "0":
        return Lease(0, [])
    return Pool().top_up(klass, int(held), want)


def acquire(
    klass: str,
    want: int,
    give_up: Callable[[], bool] = lambda: False,
    command: str = "",
) -> Lease | None:
    """A lease from the machine's pool (`Pool.acquire`)."""
    return Pool().acquire(klass, want, give_up, command)


def lease(
    klass: str, want: int, give_up: Callable[[], bool], command: str = ""
) -> Lease | None:
    """A check step's lease (`acquire`): with `want` zero, an empty one that holds the step to
    nothing, at once."""
    return acquire(klass, want, give_up, command) if want else Lease(0, [])


def named_class() -> str:
    """The class a command takes tokens as when it holds none: that of the check step it runs
    under, which names its class even when it holds no tokens (`CLASS_ENV`), and otherwise
    `agent`."""
    named = os.environ.get(CLASS_ENV, "")
    return named if named in CLASSES else "agent"


def run_program(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[Any]:
    """Runs `command`, a program that spreads its work over as many threads as its lease has
    tokens (`LEASE_ENV`), inside a lease of [`named_class`], or inside the lease its parent
    already holds."""
    lease = acquire(named_class(), largest(pool_size()), command=Path(command[0]).name)
    assert lease is not None
    with lease:
        env = {**(kwargs.pop("env", None) or os.environ), **lease.env()}
        return subprocess.run(command, env=env, **kwargs)


def script_class(script: str) -> str:
    """The class of a check run's steps: `check` is the fast check of a push, or of a batch of
    them in the merge queue's tree, `ci` is slow CI, and a sweep's script (`SWEEP_SCRIPTS`) is
    `sweep`; any other run is a session's own.
    """
    if script == "check" and paths.REPO.name == QUEUE_TREE:
        return "queue"
    if script in SWEEP_SCRIPTS:
        return "sweep"
    return {"check": "push", "ci": "ci"}.get(script, "agent")


# What a lease or low priority puts in a command's environment (`Lease.env`, `system.LOW_ENV`).
HELD_ENV: tuple[str, ...] = (
    LEASE_ENV,
    CLASS_ENV,
    "CARGO_BUILD_JOBS",
    "NEXTEST_TEST_THREADS",
    "RUST_TEST_THREADS",
    *system.LOW_ENV,
)


def unheld(env: dict[str, str]) -> dict[str, str]:
    """`env` without what a lease or low priority set in it ([`HELD_ENV`]), for a command that
    takes its own tokens, such as a sweep that slow CI starts from inside a step's lease.
    """
    return {k: v for k, v in env.items() if k not in HELD_ENV}


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
    lease = acquire(
        args.klass,
        args.cores or largest(pool_size()),
        command=Path(command[0]).name,
    )
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
