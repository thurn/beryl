"""Cargo unit seconds, excluding token waits, build locks and cache copying.

Reads tools' documented compile_timings.py JSON lines without importing tools.
Crate totals sum all targets and build scripts in each invocation. A report's
share is of summed crate seconds in that mode, not elapsed lane wall time:
parallel units overlap. Fresh invocations are counted separately from rebuilt
samples, so a warm cache cannot make an expensive crate's median disappear.
"""

from __future__ import annotations

import json
import math
import sqlite3
from collections import defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from statistics import median

from . import metrics
from .cilog import host_name, iso, utc
from .facts import Facts
from .ingest import LineSource, Row
from .machine import bounds
from .render import Result, table
from .scope import Scope

MODES = ("check", "clippy", "test-build")


class Timings(LineSource):
    """One immutable JSON-lines file per Cargo invocation, written only on success."""

    name = "timings"

    def __init__(self, cache: Path) -> None:
        self.folder: Path = cache / "ci" / "compile-timings"

    def files(self) -> list[Path]:
        return sorted(self.folder.glob("*.jsonl"))

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        mode = record["mode"]
        seconds = record["seconds"]
        fresh = record["fresh"]
        if mode not in MODES or not isinstance(fresh, bool):
            raise ValueError("invalid mode or freshness")
        if isinstance(seconds, bool) or not isinstance(seconds, (float, int)):
            raise TypeError("seconds is not a number")
        if not math.isfinite(seconds) or seconds < 0 or (fresh and seconds != 0):
            raise ValueError("invalid unit duration")
        return [
            (
                "compile_timings",
                {
                    "host": host_name(record["host"]),
                    "run": str(record["run"]),
                    "time": iso(utc(record["time"])),
                    "crate": str(record["crate"]),
                    "mode": mode,
                    "seconds": float(seconds),
                    "fresh": int(fresh),
                },
            )
        ]


@dataclass
class Cost:
    crate: str
    mode: str
    builds: int
    fresh: int
    median_seconds: float | None
    seconds: float
    share: float


def report(db: sqlite3.Connection, scope: Scope) -> Result:
    clause, given = bounds("time", scope)
    groups: dict[tuple[str, str], list[tuple[float, bool]]] = defaultdict(list)
    totals: dict[str, float] = defaultdict(float)
    for crate, mode, seconds, fresh in db.execute(
        "SELECT crate, mode, seconds, fresh FROM compile_timings WHERE 1" + clause,
        given,
    ):
        groups[(str(crate), str(mode))].append((float(seconds), bool(fresh)))
        totals[str(mode)] += float(seconds)
    rows = []
    for (crate, mode), samples in groups.items():
        rebuilt = [s for s, fresh in samples if not fresh]
        seconds = sum((s for s, _ in samples), 0.0)
        rows.append(
            Cost(
                crate,
                mode,
                len(rebuilt),
                sum(f for _, f in samples),
                round(median(rebuilt), 3) if rebuilt else None,
                round(seconds, 3),
                round(100 * seconds / totals[mode], 2) if totals[mode] else 0.0,
            )
        )
    rows.sort(key=lambda r: (-(r.median_seconds or 0), r.crate, r.mode))
    return Result(
        "compile",
        {"since": scope.range.since, "costs": [asdict(r) for r in rows]},
        [
            "Cargo crate seconds; shares use summed unit seconds within each mode.",
            "Fresh cache hits are excluded from build medians; waits and cache copying are excluded.",
            *table(
                ["crate", "mode", "builds", "fresh", "median s", "sum s", "share %"],
                [
                    [
                        r.crate,
                        r.mode,
                        r.builds,
                        r.fresh,
                        r.median_seconds,
                        r.seconds,
                        r.share,
                    ]
                    for r in rows
                ],
            ),
        ],
        [],
    )


def points(
    crate: str, mode: str, db: sqlite3.Connection, _: Facts, scope: Scope
) -> metrics.Points:
    clause, given = bounds("time", scope)
    return [
        (str(t), float(s))
        for t, s in db.execute(
            "SELECT time, seconds FROM compile_timings WHERE crate = ? AND mode = ? AND fresh = 0"
            + clause
            + " ORDER BY time",
            (crate, mode, *given),
        )
    ]


def register_metrics(db: sqlite3.Connection) -> None:
    """Discover crate names in this store; register all three modes for each crate."""
    from functools import partial

    for (crate,) in db.execute(
        "SELECT DISTINCT crate FROM compile_timings ORDER BY crate"
    ):
        for mode in MODES:
            name = f"compile.{crate}.{mode}"
            # Concurrent page queries may discover the same crate together.
            metrics.METRICS.setdefault(
                name,
                metrics.Metric(
                    name,
                    f"rebuilt {crate} unit seconds ({mode})",
                    "seconds",
                    3,
                    partial(points, str(crate), mode),
                ),
            )
