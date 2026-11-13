"""This machine's speed against the reference machine the check budgets were set on, so a
budget means the same work on every machine (checks.py, clock.py).

`by speed` times a fixed CPU workload spread over every core, the way a build or a test run
spreads, and saves the time in the machine's cache ([`CACHE_FILE`]). The checks' clock divides
each second by `factor()`, this machine's time over the reference's
(`data/speed-reference.json`), the same way it divides by the load: on a machine half as fast,
two wall seconds count as one budget second. A machine with no saved time counts as the
reference (factor 1).

The machine need not be quiet. Each round is timed the way the checks' clock times a step: the
wall time divided by the load's stretch (`clock.stretch`), counting the load from before the
round plus the cores the workload itself occupies, because the load average lags behind a burst
that short. The rounds are spread over about a minute and the best one is kept, which is the
one the least other work disturbed. Only a load past [`BUSIEST`] times the cores, where the
clock's stretch stops growing, is refused.

The first check run on a machine with no saved time starts `by speed --background` and goes
on (`start_background`); the factor applies from the next run. `by speed --reference` writes
the reference instead; run it on the machine the budgets are set on, then commit
`data/speed-reference.json`. A changed workload invalidates every saved time: bump
[`WORKLOAD`] and measure again.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import multiprocessing
import os
import platform
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

from . import paths, system

# The workload's version: a saved time of another version is ignored.
WORKLOAD = 2
# Tasks per round, many more than any machine's cores, so faster cores take more of them.
TASKS = 480
ROUNDS = 5
# Seconds between rounds, so that a burst of other work spoils one round, not all of them.
GAP = 12.0
# The most load, in multiples of the cores, a measurement is taken under. Past this the
# checks' clock stops stretching (clock.MAX_STRETCH), so no adjustment could be trusted.
BUSIEST = 4.0
CACHE_FILE = "speed.json"
# Marks a background measurement in progress, so that parallel check runs start one.
PENDING_FILE = "speed.pending"
# A pending mark older than this is a measurement that died; the next run starts another.
PENDING_SECONDS = 600
OFF_ENV = "BERYL_SPEED"
REFERENCE: Path = paths.REPO / "data" / "speed-reference.json"
# A factor outside these is a broken measurement, not a machine.
LIMITS = (0.25, 4.0)


def task(seed: int) -> int:
    """A slice of the workload: hashing and integer arithmetic, about 30 ms on an M1 Max core."""
    digest = seed.to_bytes(8, "little")
    for _ in range(30000):
        digest = hashlib.sha256(digest).digest()
    total = 0
    for i in range(200000):
        total = (total * 31 + i ^ digest[i % 32]) % 1_000_003
    return total


def adjusted(wall: float, load_before: float, cores: int) -> float:
    """A round's wall seconds as an otherwise idle machine would have spent them: the load
    before the round is other work, and the workload adds one runnable process per core.
    """
    from .clock import stretch  # The clock imports this module.

    return wall / stretch(load_before + cores, cores)


def measure(rounds: int = ROUNDS, gap: float = GAP) -> tuple[float, float]:
    """The best round's load-adjusted seconds, and the load average before that round."""
    cores = os.cpu_count() or 1
    best, best_load = float("inf"), 0.0
    with multiprocessing.Pool(cores) as pool:
        pool.map(task, range(cores))  # Start every worker first.
        for i in range(rounds):
            if i:
                time.sleep(gap)
            load = system.loadavg()[0]
            started = time.perf_counter()
            for _ in pool.imap_unordered(task, range(TASKS), chunksize=1):
                pass
            seconds = adjusted(time.perf_counter() - started, load, cores)
            if seconds < best:
                best, best_load = seconds, load
    return best, best_load


def cpu() -> str:
    if sys.platform == "darwin":
        run = subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"],
            capture_output=True,
            text=True,
            check=False,
        )
        if run.stdout.strip():
            return run.stdout.strip()
    return platform.processor() or platform.machine()


def read(path: Path) -> float | None:
    """A saved time's seconds, if the file holds one of this workload."""
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    if not isinstance(data, dict) or data.get("workload") != WORKLOAD:
        return None
    seconds = data.get("seconds")
    return float(seconds) if isinstance(seconds, (int, float)) and seconds > 0 else None


def factor() -> float:
    """How many times longer this machine takes than the reference: 1 when either has no
    saved time."""
    mine = read(paths.cache_root() / CACHE_FILE)
    reference = read(REFERENCE)
    if mine is None or reference is None:
        return 1.0
    low, high = LIMITS
    return min(high, max(low, mine / reference))


def start_background() -> str | None:
    """Starts `by speed --background` when this machine has no saved time and none is being
    measured; returns a line saying so, for the check's output."""
    cache = paths.cache_root()
    if os.environ.get(OFF_ENV) == "off" or read(cache / CACHE_FILE) is not None:
        return None
    pending = cache / PENDING_FILE
    try:
        if time.time() - pending.stat().st_mtime < PENDING_SECONDS:
            return None
    except OSError:
        pass
    pending.write_text(str(os.getpid()))
    with open(cache / "speed.log", "w") as log:
        subprocess.Popen(
            [sys.executable, "-m", "beryl_tools.cli", "speed", "--background"],
            stdout=log,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
            cwd=paths.TOOLS,
        )
    return "speed: this machine has no saved speed; measuring it in the background (`just speed`)"


def speed(args: argparse.Namespace) -> int:
    cores = os.cpu_count() or 1
    pending = paths.cache_root() / PENDING_FILE
    try:
        load = system.loadavg()[0]
        if load > BUSIEST * cores and not args.force:
            print(
                f"speed: load average {load:.1f} on {cores} cores, past {BUSIEST:.0f}x the"
                " cores: no adjustment for load is trustworthy there (--force)",
                file=sys.stderr,
            )
            return 2
        seconds, under = measure()
    finally:
        if args.background:
            pending.unlink(missing_ok=True)
    record = {
        "workload": WORKLOAD,
        "seconds": round(seconds, 3),
        "load": round(under, 1),
        "cpu": cpu(),
        "cores": cores,
        "measured": datetime.now(timezone.utc).strftime("%Y-%m-%d"),
    }
    target = REFERENCE if args.reference else paths.cache_root() / CACHE_FILE
    target.write_text(json.dumps(record, indent=2) + "\n")
    print(
        f"speed: {seconds:.2f} s on {record['cpu']} ({cores} cores), best round under load"
        f" average {under:.1f}, written to {target}"
    )
    if not args.reference:
        print(f"speed: factor {factor():.2f} against the reference")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "speed", help="measure this machine's speed for the checks' budgets"
    )
    p.add_argument(
        "--reference",
        action="store_true",
        help="write the reference (data/speed-reference.json) instead of this machine's",
    )
    p.add_argument(
        "--force", action="store_true", help="measure past the load it refuses"
    )
    p.add_argument(
        "--background",
        action="store_true",
        help="as started by a check run (clears its pending mark)",
    )
    p.set_defaults(func=speed)
