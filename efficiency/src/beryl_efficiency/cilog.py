"""Helpers for the CI rows the store keeps (docs/efficiency-dashboard.md, "CI accounting"):
the `checks` and `lane_runs` rows that Ignite's records fill (`ignite.py`), and the
`verdicts` rows from before the switch, which the store keeps.

A check belongs to a family ([`family`]), which groups related checks in the tree the `ci`
query shows: script, check lane, family, check.
"""

from __future__ import annotations

from datetime import datetime, timezone

from . import scope
from .ingest import Merge

VERDICTS = ("catch", "stale", "flaky", "mixed", "miss")


def family(check: str) -> str:
    """The family of a check, from its name: `rust:<binary>` for a Rust test (the text up to
    the first `::`), `oracle:<engine>` for an oracle scenario (the text after the last `/`),
    and otherwise the text before the first `:`, or the whole name if it has none."""
    if check.startswith("rust:"):
        return check.split("::", 1)[0]
    if check.startswith("oracle:") and "/" in check:
        return "oracle:" + check.rsplit("/", 1)[1]
    return check.split(":", 1)[0]


def host_name(value: object) -> str:
    """A host as every row of the store records it (paths.host): the node name without a
    domain, in lower case."""
    return str(value or "").split(".")[0].lower() or "unknown"


def iso(moment: datetime) -> str:
    return moment.isoformat(timespec="milliseconds").replace("+00:00", "Z")


def utc(value: object) -> datetime:
    """A recorded time as a UTC datetime. Raises ValueError or TypeError for one
    that is not a time."""
    if not isinstance(value, str):
        raise TypeError("time is not text")
    found = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if found.tzinfo is None:
        found = found.replace(tzinfo=timezone.utc)
    return found.astimezone(timezone.utc)


def local_day(moment: datetime) -> str:
    """The local day of a UTC time, as rollups.day_of gives it."""
    return scope.local_time(moment).strftime("%Y-%m-%d")


def number(value: object) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value)


CHECK_SUMS: Merge = {
    "first": "min",
    "runs": "sum",
    "passes": "sum",
    "fails": "sum",
    "skips": "sum",
    "seconds": "sum",
    "cpu_seconds": "sum",
    "measured_seconds": "sum",
}
LANE_RUN_SUMS: Merge = {
    "start": "min",
    "end": "max",
    "checks": "sum",
    "fails": "sum",
    "seconds": "sum",
    "cpu_seconds": "sum",
    "measured_seconds": "sum",
}
