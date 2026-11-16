"""Where the run log (runlog.py) keeps its lines, and the sums of them that the fast check reads.

The run log used to be one file, `$BERYL_CACHE/ci/runs.jsonl`, that nothing rotated. By
2026-11-16 it held 1.4 GB and grew about 60 MB an hour, mostly from the per-test records of
Beryl 3D's interface tests. Every fast-check lane scanned all of it twice, to time and order its
checks (selection.Chooser and deadline.Records): 2.3-3.6 s and 130-370 MB a read on this Mac,
and more with every day of history. Those readers only need each check's totals over 30 days
(runlog.tally), so the log is now kept in hour files with per-lane sums beside them:

- `ci/runs/<YYYY-MM-DDTHH>.jsonl` holds the lines appended in that UTC hour. A line goes to the
  hour in which it was appended, not the hour of its `time`, so an hour that is over gets no
  more lines and its sums never go stale.
- `ci/sums/<YYYY-MM-DDTHH>/<lane>.json` and `ci/sums/<YYYY-MM-DD>/<lane>.json` hold each
  check's counts (`Tally.counts`) over one hour's or one day's lines of that lane, with the
  reruns of a bisection left out, as runlog.tally leaves them out. A directory is complete once
  it holds [`DONE`]. An hour is summed the first time a reader needs it after the hour ends. A
  day is summed from its hours once the day is over, and its hours' sums are then deleted.
- `ci/runs.jsonl` is the old single file. Tools from before the split, in worktrees that have
  not rebased, still append to it, so the next append folds its lines into
  `ci/runs/<YYYY-MM-DDTHH>.folded.jsonl` by the hour of their `time`, and drops the sums those
  hours and their days already had. The efficiency dashboard reads the old file itself, so it
  skips the folded files rather than count their lines twice.

Appending, folding and building sums all hold `ci/runs.lock`, so a sum never misses a line
that an append was writing. Hour files are kept [`RAW_DAYS`] days and sums [`SUM_DAYS`] days.
"""

from __future__ import annotations

import json
import os
import shutil
from collections.abc import Iterable, Iterator
from contextlib import AbstractContextManager
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import paths, system

# A bisection's reruns at older commits (bisect.py), which say nothing of a check's own record.
BISECT = "bisect"
# A record backfilled from history of a commit that rewrote a check (runlog.REWRITE).
REWRITE = "rewrite"
# The file that marks a sums directory as complete.
DONE = "DONE"
# How long hour files are kept: the longest window a reader asks for (selection.RECORD_DAYS,
# check_value.DAYS and bisect.DAYS are 30) and a day for the partial one at its start.
RAW_DAYS = 31
SUM_DAYS = 31
HOUR = "%Y-%m-%dT%H"
DAY = "%Y-%m-%d"
# The lines a fold holds in memory for one hour before writing them out.
FOLD_BATCH = 20_000


@dataclass
class Tally:
    """A check's or a lane's runs, failures, skips and seconds in the run log, with the
    verdicts on its failures (runlog.tally)."""

    runs: int = 0
    failures: int = 0
    skips: int = 0
    seconds: float = 0.0
    verdicts: dict[str, int] = field(default_factory=dict)
    backfilled: int = 0

    def add(self, r: dict[str, object]) -> None:
        outcome = r.get("outcome")
        self.backfilled += bool(r.get("backfilled"))
        if outcome == REWRITE:
            self.judged("stale")
            return
        if outcome == "skip":
            self.skips += 1
            return
        self.runs += 1
        self.failures += outcome == "fail"
        seconds = r.get("seconds")
        self.seconds += seconds if isinstance(seconds, (int, float)) else 0.0

    def judged(self, verdict: str, backfilled: bool = False) -> None:
        self.verdicts[verdict] = self.verdicts.get(verdict, 0) + 1
        self.backfilled += backfilled

    def json(self) -> dict[str, object]:
        return {
            "runs": self.runs,
            "failures": self.failures,
            "skips": self.skips,
            "seconds": round(self.seconds, 1),
            "verdicts": self.verdicts,
            "backfilled": self.backfilled,
        }

    def merge(self, other: Tally) -> None:
        self.runs += other.runs
        self.failures += other.failures
        self.skips += other.skips
        self.seconds += other.seconds
        self.backfilled += other.backfilled
        for verdict, n in other.verdicts.items():
            self.verdicts[verdict] = self.verdicts.get(verdict, 0) + n

    def counts(self) -> list[object]:
        """What a sums file keeps of it: runs, failures, skips, seconds, backfilled, and the
        verdicts the log itself gives (only `stale`, from a rewrite record)."""
        return [
            self.runs,
            self.failures,
            self.skips,
            round(self.seconds, 3),
            self.backfilled,
            self.verdicts,
        ]

    @staticmethod
    def of(counts: list[object]) -> Tally:
        runs, failures, skips, seconds, backfilled, verdicts = counts
        assert isinstance(runs, int) and isinstance(failures, int)
        assert isinstance(skips, int) and isinstance(backfilled, int)
        assert isinstance(seconds, (int, float)) and isinstance(verdicts, dict)
        return Tally(
            runs,
            failures,
            skips,
            float(seconds),
            {str(k): int(v) for k, v in verdicts.items()},
            backfilled,
        )


Sums = dict[str, dict[str, Tally]]


def root() -> Path:
    return paths.cache_root() / "ci"


def legacy_path() -> Path:
    """The old single run log, which tools from before the hour files append to."""
    return root() / "runs.jsonl"


def hours_dir() -> Path:
    return root() / "runs"


def sums_dir() -> Path:
    return root() / "sums"


def lock() -> AbstractContextManager[None, bool | None]:
    root().mkdir(parents=True, exist_ok=True)
    return system.file_lock(root() / "runs.lock")


def utc_now() -> datetime:
    return datetime.now(timezone.utc)


def hour_file(key: str) -> Path:
    return hours_dir() / f"{key}.jsonl"


def folded_file(key: str) -> Path:
    """Where `fold` puts the old single file's lines whose `time` falls in the hour."""
    return hours_dir() / f"{key}.folded.jsonl"


def hour_lines(key: str, needle: bytes = b"") -> Iterator[bytes]:
    """The lines of an hour's files that contain `needle`."""
    for path in (hour_file(key), folded_file(key)):
        try:
            with open(path, "rb") as f:
                yield from (line for line in f if needle in line)
        except FileNotFoundError:
            continue


def append(text: str) -> None:
    """Appends lines to the current hour's file, folding the old single file in first."""
    with lock():
        fold()
        hours_dir().mkdir(parents=True, exist_ok=True)
        with open(hour_file(utc_now().strftime(HOUR)), "a") as f:
            f.write(text)


def record_hour(line: bytes) -> str | None:
    """The UTC hour of a line's `time`; None for a line that is not a record."""
    try:
        r = json.loads(line)
        when = datetime.fromisoformat(r["time"])
    except (KeyError, TypeError, ValueError):  # JSON and Unicode errors among them.
        return None
    if when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    return when.astimezone(timezone.utc).strftime(HOUR)


def fold() -> int:
    """Moves the old single file's lines into the hour files of their `time`, under the lock,
    and drops the sums of each hour and day it added to. Returns the lines it moved."""
    old = legacy_path()
    try:
        if old.stat().st_size == 0:
            return 0
    except FileNotFoundError:
        return 0
    moving = root() / f"runs.folding.{os.getpid()}"
    old.rename(moving)
    hours_dir().mkdir(parents=True, exist_ok=True)
    pending: dict[str, list[bytes]] = {}
    touched: set[str] = set()
    moved = 0

    def flush(key: str) -> None:
        with open(folded_file(key), "ab") as f:
            f.writelines(pending.pop(key))

    with open(moving, "rb") as f:
        for line in f:
            key = record_hour(line)
            if key is None:
                continue
            if not line.endswith(b"\n"):
                line += b"\n"
            pending.setdefault(key, []).append(line)
            touched.add(key)
            moved += 1
            if len(pending[key]) >= FOLD_BATCH:
                flush(key)
    for key in list(pending):
        flush(key)
    for key in touched:
        for done in (sums_dir() / key, sums_dir() / key[:10]):
            shutil.rmtree(done, ignore_errors=True)
    moving.unlink()
    return moved


def read_lines(since: datetime | None) -> Iterator[bytes]:
    """Every line of the hour files from `since`'s hour on (all of them for None), then any
    left in the old single file. A line's own `time` may be earlier than its file's hour, so
    readers still filter on it."""
    floor = "" if since is None else since.astimezone(timezone.utc).strftime(HOUR)
    files = sorted(p for p in hours_dir().glob("*.jsonl") if p.name[:13] >= floor) + [
        legacy_path()
    ]
    for path in files:
        try:
            f = open(path, "rb")
        except FileNotFoundError:  # Pruned or folded since it was listed.
            continue
        with f:
            yield from f


def lane_file(lane: str) -> str:
    return lane.replace("/", "_").replace("\\", "_") + ".json"


def count(lines: Iterable[bytes]) -> Sums:
    """Each lane's checks' tallies over `lines`, bisection reruns left out."""
    sums: Sums = {}
    for line in lines:
        try:
            r = json.loads(line)
        except ValueError:  # A torn line, or JSON and Unicode errors.
            continue
        if not isinstance(r, dict) or r.get("script") == BISECT:
            continue
        lane = str(r.get("lane"))
        sums.setdefault(lane, {}).setdefault(str(r.get("check")), Tally()).add(r)
    return sums


def write_sums(folder: Path, sums: Sums) -> None:
    """Writes a complete sums directory, replacing any partial one."""
    shutil.rmtree(folder, ignore_errors=True)
    building = folder.with_name(f"{folder.name}.building.{os.getpid()}")
    shutil.rmtree(building, ignore_errors=True)
    building.mkdir(parents=True)
    for lane, checks in sums.items():
        data = {check: t.counts() for check, t in sorted(checks.items())}
        (building / lane_file(lane)).write_text(json.dumps(data, separators=(",", ":")))
    (building / DONE).write_text("")
    building.rename(folder)


def read_sums(folder: Path, lane: str | None) -> Sums:
    """A complete sums directory's tallies, for `lane` only when given."""
    files = (
        [folder / lane_file(lane)]
        if lane is not None
        else [p for p in folder.glob("*.json")]
    )
    sums: Sums = {}
    for path in files:
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        name = lane if lane is not None else path.stem
        sums[name] = {check: Tally.of(c) for check, c in data.items()}
    return sums


def merge(into: Sums, more: Sums) -> None:
    for lane, checks in more.items():
        mine = into.setdefault(lane, {})
        for check, t in checks.items():
            mine.setdefault(check, Tally()).merge(t)


def hour_sums(key: str, lane: str | None) -> Sums:
    """An hour that is over, summed: from its sums directory, built from its file under the
    lock the first time it is needed."""
    folder = sums_dir() / key
    if not (folder / DONE).exists():
        with lock():
            if not (folder / DONE).exists():
                write_sums(folder, count(hour_lines(key)))
    return read_sums(folder, lane)


def day_sums(day: datetime, lane: str | None) -> Sums:
    """A UTC day that is over, summed from its 24 hours the first time it is needed, after
    which the hours' own sums are deleted and old files pruned."""
    key = day.strftime(DAY)
    folder = sums_dir() / key
    if not (folder / DONE).exists():
        hours = [(day + timedelta(hours=h)).strftime(HOUR) for h in range(24)]
        total: Sums = {}
        for hour in hours:
            merge(total, hour_sums(hour, None))
        with lock():
            if not (folder / DONE).exists():
                write_sums(folder, total)
                for hour in hours:
                    shutil.rmtree(sums_dir() / hour, ignore_errors=True)
        prune()
    return read_sums(folder, lane)


def prune(now: datetime | None = None) -> None:
    """Deletes hour files older than RAW_DAYS and sums older than SUM_DAYS."""
    now = now or utc_now()
    raw = (now - timedelta(days=RAW_DAYS)).strftime(HOUR)
    for p in hours_dir().glob("*.jsonl"):
        if p.name[:13] < raw:
            p.unlink(missing_ok=True)
    old = (now - timedelta(days=SUM_DAYS)).strftime(DAY)
    for p in sums_dir().glob("*"):
        if p.is_dir() and p.name[:10] < old:
            shutil.rmtree(p, ignore_errors=True)


def lane_tallies(
    since: datetime, lane: str, now: datetime | None = None
) -> dict[str, Tally]:
    """Each of `lane`'s checks' tallies from `since` on, bisection reruns left out: the sums of
    the whole days from `since`'s day to yesterday and of today's finished hours, and the lines
    of the current hour and of the old single file. `since`'s day counts whole, so the window
    can start up to a day early; the readers' windows are 30 days, so that is noise."""
    at: datetime = (now or utc_now()).astimezone(timezone.utc)
    today = at.replace(hour=0, minute=0, second=0, microsecond=0)
    total: Sums = {}
    day = since.astimezone(timezone.utc).replace(
        hour=0, minute=0, second=0, microsecond=0
    )
    while day < today:
        merge(total, day_sums(day, lane))
        day += timedelta(days=1)
    for h in range(at.hour):
        merge(total, hour_sums((today + timedelta(hours=h)).strftime(HOUR), lane))
    needle: bytes = f'"lane": {json.dumps(lane)}'.encode()
    hour: str = at.strftime(HOUR)

    def current() -> Iterator[bytes]:
        yield from hour_lines(hour, needle)
        try:
            with open(legacy_path(), "rb") as f:
                yield from (line for line in f if needle in line)
        except FileNotFoundError:
            pass

    merge(total, {lane: count(current()).get(lane, {})})
    return total.get(lane, {})


def roll() -> str:
    """Folds the old single file and builds every missing sum in the windows readers use, so
    that no fast check pays for it: `by run-log roll`, run once after the split lands.
    """
    with lock():
        moved = fold()
    now = utc_now()
    since = now - timedelta(days=RAW_DAYS - 1)
    started = datetime.now()
    # Reading any one lane builds every lane's sums for each hour and day it covers.
    lane_tallies(since, "-", now)
    took = (datetime.now() - started).total_seconds()
    return f"run log: folded {moved} lines of {legacy_path()}; summed in {took:.0f} s"
