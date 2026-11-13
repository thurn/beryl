"""This machine's speed against the reference machine the check budgets were set on, so a
budget means the same work on every machine (checks.py, clock.py).

`by speed` times a fixed CPU workload spread over every core, as a build or a test run
spreads, and keeps the time in the machine's cache ([`CACHE_FILE`]). The checks' clock divides
each second by `factor()`, this machine's time over the reference's
(`data/speed-reference.json`), as it divides by the load: a machine half as fast spends two
wall seconds per budget second. A machine never measured counts as the reference (factor 1).

`by speed --reference` measures and writes the reference instead; run it on the machine the
budgets are set on, then commit `data/speed-reference.json`. A changed workload invalidates
every measurement: bump [`WORKLOAD`] and measure again.
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

# The workload's version: a measurement of another version is ignored.
WORKLOAD = 1
# Tasks per run, many more than any machine's cores, so faster cores take more of them.
TASKS = 480
ROUNDS = 3
CACHE_FILE = "speed.json"
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


def measure() -> float:
    """Seconds the workload takes on every core, the best of [`ROUNDS`]."""
    best = float("inf")
    with multiprocessing.Pool(os.cpu_count() or 1) as pool:
        pool.map(task, range(os.cpu_count() or 1))  # Start every worker first.
        for _ in range(ROUNDS):
            started = time.perf_counter()
            for _ in pool.imap_unordered(task, range(TASKS), chunksize=1):
                pass
            best = min(best, time.perf_counter() - started)
    return best


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
    """A measurement's seconds, if the file holds one of this workload."""
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    if not isinstance(data, dict) or data.get("workload") != WORKLOAD:
        return None
    seconds = data.get("seconds")
    return float(seconds) if isinstance(seconds, (int, float)) and seconds > 0 else None


def factor() -> float:
    """How many times longer this machine takes than the reference: 1 when either is
    unmeasured."""
    mine = read(paths.cache_root() / CACHE_FILE)
    reference = read(REFERENCE)
    if mine is None or reference is None:
        return 1.0
    low, high = LIMITS
    return min(high, max(low, mine / reference))


def speed(args: argparse.Namespace) -> int:
    load = system.loadavg()[0]
    cores = os.cpu_count() or 1
    if load > cores / 4 and not args.force:
        print(
            f"speed: load average {load:.1f} on {cores} cores: measure on a quiet machine"
            " (--force to measure anyway)",
            file=sys.stderr,
        )
        return 2
    seconds = measure()
    record = {
        "workload": WORKLOAD,
        "seconds": round(seconds, 3),
        "cpu": cpu(),
        "cores": cores,
        "measured": datetime.now(timezone.utc).strftime("%Y-%m-%d"),
    }
    target = REFERENCE if args.reference else paths.cache_root() / CACHE_FILE
    target.write_text(json.dumps(record, indent=2) + "\n")
    print(
        f"speed: {seconds:.2f} s on {record['cpu']} ({cores} cores), written to {target}"
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
    p.add_argument("--force", action="store_true", help="measure on a busy machine")
    p.set_defaults(func=speed)
