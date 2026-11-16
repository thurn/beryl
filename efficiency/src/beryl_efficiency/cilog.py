"""CI's sources (docs/efficiency-dashboard.md, "CI accounting"): the run log, and the verdicts
on the checks' beads. Both are written by the repository's tools and read here by their
documented formats:

- The run log (tools/src/beryl_tools/runlog.py and runstore.py), the hour files
  `<cache>/ci/runs/<YYYY-MM-DDTHH>.jsonl` and the old single file `<cache>/ci/runs.jsonl`, has
  one line per check per run:
  `time` (the end of its step), `host`, `worktree` (the checkout's directory name, absent in
  lines from before it was recorded), `commit`, `tree`, `script` (`check`, `ci`, `sweep`,
  `nightly`, `bisect` and others), `lane`, `check`, `outcome` (`pass`, `fail`, `skip`,
  `pending-pass` or `pending-fail`), `seconds` and, where it was measured, `cpu_seconds`. A line
  marked `backfilled`, or with the outcome `rewrite`, records history rather than a run and is
  left out. The file is hundreds of megabytes, so its lines are added up as they are read into
  one `checks` row per check per day and one `lane_runs` row per run of a check lane; no line
  is kept.
- The verdicts are JSON comments on the beads labeled `ci-check`, one bead per check that has
  ever failed, titled with the check (tools/src/beryl_tools/verdicts.py, `entries`). A comment
  holds the failure's `time`, `host`, `commit`, `script`, `lane`, its `verdict` (`catch`,
  `stale`, `flaky`, `mixed` or `miss`) and, when a Steward bead fixed it, `steward`. A later
  comment with `corrects` set to an earlier comment's id replaces that one's verdict. They are
  read from `bd export`, which prints every bead with its comments.

A check belongs to a family ([`family`]), which groups related checks in the tree the `ci`
query shows: script, check lane, family, check.
"""

from __future__ import annotations

import json
from collections.abc import Callable, Mapping
from datetime import datetime, timedelta, timezone
from pathlib import Path

from .beads import main_checkout, run_bd
from .ingest import LineSource, Merge, Row, SnapshotSource, SourceError

EXPORT: list[str] = ["bd", "export"]
# The label of the beads that hold each check's verdicts.
CHECK_LABEL = "ci-check"
# The outcomes of a line that ran a check; `skip` is the only other one counted.
RAN: frozenset[str] = frozenset({"pass", "fail", "pending-pass", "pending-fail"})
# The verdicts a failure can get.
VERDICTS: tuple[str, ...] = ("catch", "stale", "flaky", "mixed", "miss")


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
    """A run-log or verdict time as a UTC datetime. Raises ValueError or TypeError for one
    that is not a time."""
    if not isinstance(value, str):
        raise TypeError("time is not text")
    found = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if found.tzinfo is None:
        found = found.replace(tzinfo=timezone.utc)
    return found.astimezone(timezone.utc)


def local_day(moment: datetime) -> str:
    """The local day of a UTC time, as rollups.day_of gives it."""
    return moment.astimezone().strftime("%Y-%m-%d")


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
    """The `verdicts` rows of one `ci-check` bead, each corrected by any later comment that
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
    """The verdicts on the checks' beads, from `bd export`."""

    name = "verdicts"
    owns = ("verdicts",)

    def __init__(self, read: Callable[[], bytes]) -> None:
        self._read = read

    @staticmethod
    def here(start: Path) -> Verdicts:
        """The source for the repository at `start`, read with `bd` in its main checkout."""
        checkout = main_checkout(start)
        return Verdicts(lambda: run_bd(checkout, EXPORT))

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
            raise SourceError("bd export printed no beads")
        return rows, skipped


def runlog_path(cache: Path) -> Path:
    """The run log's old single file (runstore.legacy_path in tools); its hour files are in
    `runs/` beside it."""
    return cache / "ci" / "runs.jsonl"
