"""Stand-ins for the sources that Ignite's hub replaced, which write rows in the shape the
stored history has. Tests build stores with them: a ticket snapshot, the run log, the CPU
sampler's rows, the token pool's leases and the verdict comments on `ci-check` tickets. The
formats are the ones the old tools wrote (docs/efficiency-dashboard.md, "Sources"); no
production code reads them now."""

from __future__ import annotations

import json
import sqlite3
from collections.abc import Callable, Mapping
from datetime import timedelta
from pathlib import Path

from beryl_efficiency import kinds, timing
from beryl_efficiency.beads import rows_of
from beryl_efficiency.cilog import (
    CHECK_SUMS,
    LANE_RUN_SUMS,
    host_name,
    iso,
    local_day,
    number,
    utc,
)
from beryl_efficiency.cpu import from_epoch, join, parse
from beryl_efficiency.ingest import (
    LineSource,
    Merge,
    Row,
    SnapshotSource,
    SourceError,
)

CHECK_LABEL = "ci-check"
RAN: frozenset[str] = frozenset({"pass", "fail", "pending-pass", "pending-fail"})


class Beads(SnapshotSource):
    name = "beads"
    owns = ("beads", "labels", "ticket_aliases")

    def __init__(self, read: Callable[[], bytes]) -> None:
        self._read = read

    def read(self) -> bytes:
        return self._read()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        try:
            records = json.loads(data)
        except ValueError as e:
            raise SourceError(f"the snapshot contains no JSON: {e}") from e
        if not isinstance(records, list):
            raise SourceError("the snapshot contains no list of tickets")
        rows: list[Row] = []
        skipped = 0
        for record in records:
            try:
                rows += rows_of(record)
            except (TypeError, KeyError):
                skipped += 1
        return rows, skipped


class RunLog(LineSource):
    """The run log, added up into `checks` and `lane_runs`."""

    name = "runlog"
    merges: dict[str, Merge] = {"checks": CHECK_SUMS, "lane_runs": LANE_RUN_SUMS}

    def __init__(self, path: Path) -> None:
        self.path = path

    def files(self) -> list[Path]:
        """The old single file while tools from before the split still write it, then the
        hour files beside it (runstore.py in tools), but the `.folded.jsonl` ones: those hold
        lines moved out of the single file, which were read from there already."""
        hours = sorted(
            p
            for p in (self.path.parent / "runs").glob("*.jsonl")
            if not p.name.endswith(".folded.jsonl")
        )
        return ([self.path] if self.path.exists() else []) + hours

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        if not isinstance(record, dict):
            raise TypeError("not an object")
        if record.get("backfilled") or record.get("outcome") == "rewrite":
            return []
        outcome = str(record["outcome"])
        if outcome not in RAN and outcome != "skip":
            raise ValueError(f"unknown outcome {outcome}")
        ended = utc(record["time"])
        seconds = number(record["seconds"])
        if seconds is None:
            raise TypeError("seconds is not a number")
        ran = outcome in RAN
        cpu = number(record.get("cpu_seconds")) if ran else None
        host = host_name(record.get("host"))
        script = str(record["script"])
        lane = str(record["lane"])
        common = {
            "seconds": seconds if ran else 0.0,
            "cpu_seconds": cpu or 0.0,
            "measured_seconds": seconds if cpu is not None else 0.0,
        }
        return [
            (
                "checks",
                {
                    "day": local_day(ended),
                    "host": host,
                    "script": script,
                    "lane": lane,
                    "check": str(record["check"]),
                    "first": iso(ended),
                    "runs": int(ran),
                    "passes": int(outcome == "pass"),
                    "fails": int(outcome == "fail"),
                    "skips": int(outcome == "skip"),
                    **common,
                },
            ),
            (
                "lane_runs",
                {
                    "host": host,
                    "script": script,
                    "worktree": str(record.get("worktree") or ""),
                    "commit": str(record.get("commit") or ""),
                    "tree": str(record.get("tree") or ""),
                    "lane": lane,
                    "start": iso(ended - timedelta(seconds=seconds)),
                    "end": iso(ended),
                    "checks": int(ran),
                    "fails": int(outcome == "fail"),
                    **common,
                },
            ),
        ]


def verdict_rows(
    issue: dict[str, object], comment_aliases: Mapping[str, str] | None = None
) -> list[Row]:
    """The `verdicts` rows of one `ci-check` ticket, each corrected by any later comment that
    names it, as verdicts.py's `entries` reads them; comments that are not verdicts are
    left out."""
    check = issue.get("title")
    if not isinstance(check, str):
        raise TypeError("title is not text")
    comments = issue.get("comments") or []
    if not isinstance(comments, list):
        raise TypeError("comments is not a list")
    found: dict[str, dict[str, object]] = {}
    for comment in comments:
        if not isinstance(comment, dict):
            continue
        try:
            entry = json.loads(str(comment.get("text", "")))
        except ValueError:
            continue
        if not isinstance(entry, dict) or not isinstance(entry.get("verdict"), str):
            continue
        if "corrects" in entry:
            key = str(entry["corrects"])
            if comment_aliases is not None:
                key = comment_aliases.get(key, key)
            corrected = found.get(key)
            if corrected is not None:
                corrected["verdict"] = entry["verdict"]
            continue
        try:
            failed = iso(utc(entry.get("time")))
        except (ValueError, TypeError):
            continue
        key = str(comment.get("id", ""))
        found[key] = {
            "comment": key,
            "check": check,
            "time": failed,
            "host": host_name(entry.get("host")),
            "commit": str(entry.get("commit") or ""),
            "script": str(entry.get("script") or ""),
            "lane": str(entry.get("lane") or check.split(":", 1)[0]),
            "verdict": str(entry["verdict"]),
            "steward": str(entry["steward"]) if entry.get("steward") else None,
        }
    return [("verdicts", values) for values in found.values()]


class Verdicts(SnapshotSource):
    """The verdicts on the checks' tickets, from `tk export`."""

    name = "verdicts"
    owns = ("verdicts",)

    def __init__(self, read: Callable[[], bytes]) -> None:
        self._read = read

    def read(self) -> bytes:
        return self._read()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        rows: list[Row] = []
        skipped = 0
        for line in data.splitlines():
            if not line.strip():
                continue
            try:
                issue = json.loads(line)
                if not isinstance(issue, dict):
                    raise TypeError("not an object")
                labels = issue.get("labels") or []
                if isinstance(labels, list) and CHECK_LABEL in labels:
                    rows += verdict_rows(issue)
            except (ValueError, TypeError):
                skipped += 1
        if skipped and not rows:
            raise SourceError("tk export printed no tickets")
        return rows, skipped


class Samples(LineSource):
    """The CPU sampler's rows."""

    name = "cpu"

    def __init__(self, path: Path, host: str) -> None:
        self.path = path
        self.host = host

    def files(self) -> list[Path]:
        return [self.path] if self.path.exists() else []

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        time = from_epoch(float(record["time"]))
        seconds = float(record["seconds"])
        used: Mapping[str, object] = record["cpu"]
        rows: list[Row] = [
            (
                "cpu_minutes",
                {
                    "host": self.host,
                    "time": time,
                    "seconds": seconds,
                    "cores": int(record["cores"]),
                    "cpu_seconds": sum(float(str(v)) for v in used.values()),
                    "peak_cores": float(record.get("peak_cores") or 0),
                },
            )
        ]
        for key, value in used.items():
            owner, kind, worktree = key.split("/", 2)
            rows.append(
                (
                    "cpu",
                    {
                        "host": self.host,
                        "time": time,
                        "owner": owner,
                        "kind": kind,
                        "worktree": "" if worktree == "-" else worktree,
                        "seconds": seconds,
                        "cpu_seconds": float(str(value)),
                    },
                )
            )
        return rows


class Leases(LineSource):
    """The CPU token pool's log of leases."""

    name = "leases"

    def __init__(self, path: Path, host: str) -> None:
        self.path = path
        self.host = host

    def files(self) -> list[Path]:
        return [self.path] if self.path.exists() else []

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        released = parse(str(record["time"]))
        waited = float(record["waited_s"])
        held = float(record["held_s"])
        wait_end = released - timedelta(seconds=held)
        return [
            (
                "leases",
                {
                    "host": self.host,
                    "time": iso(released),
                    "class": str(record["class"]),
                    "worktree": str(record["worktree"]),
                    "command": str(record["command"]),
                    "cores": int(record["cores"]),
                    "wanted": int(record["wanted"]),
                    "waited_seconds": waited,
                    "held_seconds": held,
                    "wait_start": iso(wait_end - timedelta(seconds=waited)),
                    "wait_end": iso(wait_end),
                },
            )
        ]

    def finish(self, db: sqlite3.Connection, changed: list[Path]) -> int:
        joined = join(db, self.host)
        table, _ = kinds.load()
        heavy = frozenset(k.kind for k in table if k.heavy)
        return sum(
            timing.account(db, self.host, provider, transcript, heavy)
            for provider, transcript in sorted(joined)
        )


def samples_path(cache: Path) -> Path:
    return cache / "cpu" / "samples.jsonl"


def leases_path(cache: Path) -> Path:
    return cache / "cpu-tokens" / "leases.jsonl"


def runlog_path(cache: Path) -> Path:
    """The run log's old single file; its hour files are in `runs/` beside it."""
    return cache / "ci" / "runs.jsonl"
