"""Ingest: reads what was added to each source since the last ingest into the store
(docs/efficiency-dashboard.md, "The store").

A source is one of three kinds:

- A `LineSource` is a set of files of lines, such as transcripts or the run log. Each file is
  read from the byte offset recorded in the `sources` table. A file whose first bytes no longer
  match their recorded fingerprint was replaced, and is read again from the start. A last line
  with no newline is left for the next ingest.
- A `SnapshotSource` is a command's whole output, such as `bd list`. It is read whole every
  time, and when its digest matches the last one it writes nothing and counts no bytes read.
- A `FileSource` is a set of small files that are rewritten in place, such as the desktop
  app's session records. A file is read whole when its size or modification time changed.

Each kind turns its input into rows, `(table, {column: value})`, written with
`INSERT OR REPLACE` so that reading the same bytes twice changes nothing. A malformed line or
record is skipped and counted, and never stops an ingest.
"""

from __future__ import annotations

import hashlib
import os
import sqlite3
import time
from abc import ABC, abstractmethod
from collections.abc import Iterable, Sequence
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from . import store

# A row for the store: its table and its columns' values.
Row = tuple[str, dict[str, object]]
# How many leading bytes of a file its fingerprint covers.
FINGERPRINT_BYTES = 256
# How many rows a `LineSource` writes per transaction. A transcript can be hundreds of
# megabytes, so its rows are written, and its offset recorded, in batches as it is read.
BATCH_ROWS = 20_000
# A line or record that raises one of these is malformed: skipped and counted.
MALFORMED: tuple[type[Exception], ...] = (ValueError, KeyError, TypeError)


class SourceError(Exception):
    """A source could not be read at all, such as a command that is not installed."""


@dataclass
class Tally:
    """What one ingest did with one source."""

    bytes_read: int = 0
    rows: int = 0
    skipped: int = 0
    error: str = ""


@dataclass
class Summary:
    """What one ingest did, by source, and what it cost."""

    started: str
    sources: dict[str, Tally] = field(default_factory=dict)
    wall_seconds: float = 0.0
    cpu_seconds: float = 0.0

    def total(self) -> Tally:
        return Tally(
            sum(t.bytes_read for t in self.sources.values()),
            sum(t.rows for t in self.sources.values()),
            sum(t.skipped for t in self.sources.values()),
        )


class LineSource(ABC):
    """Files of lines, each read from its recorded offset."""

    name: str = ""

    @abstractmethod
    def files(self) -> list[Path]:
        """The files to read, as they are now."""

    @abstractmethod
    def parse(self, path: Path, line: bytes) -> list[Row]:
        """The rows of one complete line of `path`, without its newline. Raises one of
        `MALFORMED` for a line it cannot read."""

    def finish(self, db: sqlite3.Connection, changed: list[Path]) -> int:
        """Runs after an ingest read new lines from the files `changed`, to derive rows that
        depend on more than one line. Returns how many rows it wrote."""
        return 0


class FileSource(ABC):
    """Small files rewritten in place, each read whole when its size or modification time
    changed."""

    name: str = ""

    @abstractmethod
    def files(self) -> list[Path]:
        """The files to read, as they are now."""

    @abstractmethod
    def parse(self, path: Path, data: bytes) -> list[Row]:
        """The rows of one file. Raises one of `MALFORMED` for a file it cannot read."""


class SnapshotSource(ABC):
    """A command's output, read whole and written whole when it changed."""

    name: str = ""
    # The tables this source fills alone: a changed snapshot replaces their rows, so a record
    # that left the output leaves the store.
    owns: tuple[str, ...] = ()

    @abstractmethod
    def read(self) -> bytes:
        """The output. Raises `SourceError` when it cannot be had."""

    @abstractmethod
    def parse(self, data: bytes) -> tuple[list[Row], int]:
        """The rows of the output, and how many records in it were malformed."""


Source = LineSource | FileSource | SnapshotSource


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds")


def cpu_seconds() -> float:
    """CPU seconds used by this process and the commands it has waited for."""
    t = os.times()
    return t.user + t.system + t.children_user + t.children_system


def fingerprint(head: bytes) -> str:
    """`<length>:<sha256>` of a file's first bytes; the length lets a file that was short at
    the last ingest be checked against the same bytes now."""
    return f"{len(head)}:{hashlib.sha256(head).hexdigest()}"


def same_file(path: Path, recorded: str) -> bool:
    """Whether `path` still starts with the bytes `recorded` was taken of."""
    length_text, _, _ = recorded.partition(":")
    try:
        length = int(length_text)
    except ValueError:
        return False
    with open(path, "rb") as handle:
        return fingerprint(handle.read(length)) == recorded


def position(db: sqlite3.Connection, source: str, path: str) -> tuple[int, str]:
    found = db.execute(
        "SELECT offset, fingerprint FROM sources WHERE source = ? AND path = ?",
        (source, path),
    ).fetchone()
    return (int(found[0]), str(found[1])) if found else (0, "")


def record_position(
    db: sqlite3.Connection,
    source: str,
    path: str,
    offset: int,
    print_: str,
    skipped: int,
    replace: bool = False,
) -> None:
    """Records how far `source` has read `path`, adding `skipped` to its count of malformed
    lines, or with `replace` making it the count (a snapshot is counted whole each time).
    """
    total = "excluded.skipped" if replace else "skipped + excluded.skipped"
    db.execute(
        f"""INSERT INTO sources (source, path, offset, fingerprint, skipped, updated)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT (source, path) DO UPDATE SET offset = excluded.offset,
            fingerprint = excluded.fingerprint, skipped = {total},
            updated = excluded.updated""",
        (source, path, offset, print_, skipped, now()),
    )


def write(db: sqlite3.Connection, rows: Iterable[Row]) -> int:
    """Writes `rows`, replacing any with the same key. Returns how many."""
    count = 0
    for table, values in rows:
        columns = ", ".join(f'"{c}"' for c in values)
        marks = ", ".join("?" for _ in values)
        db.execute(
            f'INSERT OR REPLACE INTO "{table}" ({columns}) VALUES ({marks})',
            list(values.values()),
        )
        count += 1
    return count


def read_lines(db: sqlite3.Connection, source: LineSource, tally: Tally) -> None:
    changed: list[Path] = []
    for path in source.files():
        try:
            if read_file_lines(db, source, path, tally):
                changed.append(path)
        except OSError as e:
            tally.error = tally.error or f"{path}: {e}"
    if changed:
        with db:
            tally.rows += source.finish(db, changed)


def read_file_lines(
    db: sqlite3.Connection, source: LineSource, path: Path, tally: Tally
) -> bool:
    """Reads the complete lines `path` gained since its recorded offset, a batch of rows at a
    time, so that no file is held in memory whole. Returns whether it read any."""
    key = str(path)
    offset, recorded = position(db, source.name, key)
    size = path.stat().st_size
    if offset and (size < offset or not same_file(path, recorded)):
        offset = 0  # Replaced: read it again from the start.
    if offset == size:
        return False
    start = offset
    with open(path, "rb") as handle:
        print_ = fingerprint(handle.read(FINGERPRINT_BYTES))
        handle.seek(offset)
        rows: list[Row] = []
        skipped = 0
        while True:
            line = handle.readline()
            if not line.endswith(b"\n"):
                break  # The end, or a last line still being written.
            offset += len(line)
            if line.strip():
                try:
                    rows += source.parse(path, line.rstrip(b"\r\n"))
                except MALFORMED:
                    skipped += 1
            if len(rows) >= BATCH_ROWS:
                save_batch(db, source.name, key, offset, print_, rows, skipped, tally)
                rows, skipped = [], 0
        save_batch(db, source.name, key, offset, print_, rows, skipped, tally)
    tally.bytes_read += offset - start
    return offset > start


def save_batch(
    db: sqlite3.Connection,
    source: str,
    key: str,
    offset: int,
    print_: str,
    rows: list[Row],
    skipped: int,
    tally: Tally,
) -> None:
    with db:
        tally.rows += write(db, rows)
        record_position(db, source, key, offset, print_, skipped)
    tally.skipped += skipped


def read_files(db: sqlite3.Connection, source: FileSource, tally: Tally) -> None:
    for path in source.files():
        key = str(path)
        try:
            status = path.stat()
            stamp = f"{status.st_size}:{status.st_mtime_ns}"
            if stamp == position(db, source.name, key)[1]:
                continue
            data = path.read_bytes()
        except OSError as e:
            tally.error = tally.error or f"{path}: {e}"
            continue
        try:
            rows, skipped = source.parse(path, data), 0
        except MALFORMED:
            rows, skipped = [], 1
        with db:
            tally.rows += write(db, rows)
            record_position(db, source.name, key, len(data), stamp, skipped, True)
        tally.bytes_read += len(data)
        tally.skipped += skipped


def read_snapshot(db: sqlite3.Connection, source: SnapshotSource, tally: Tally) -> None:
    try:
        data = source.read()
        digest = fingerprint(data)
        if digest == position(db, source.name, "")[1]:
            return
        rows, skipped = source.parse(data)
    except SourceError as e:
        tally.error = str(e)
        return
    with db:
        for table in source.owns:
            db.execute(f'DELETE FROM "{table}"')
        tally.rows += write(db, rows)
        record_position(db, source.name, "", len(data), digest, skipped, replace=True)
    tally.bytes_read += len(data)
    tally.skipped += skipped


def ingest(db: sqlite3.Connection, sources: Sequence[Source], host: str) -> Summary:
    """Reads every source into `db` and records the ingest's own cost."""
    summary = Summary(now())
    wall = time.monotonic()
    cpu = cpu_seconds()
    for source in sources:
        tally = summary.sources.setdefault(source.name, Tally())
        if isinstance(source, LineSource):
            read_lines(db, source, tally)
        elif isinstance(source, FileSource):
            read_files(db, source, tally)
        else:
            read_snapshot(db, source, tally)
    summary.wall_seconds = time.monotonic() - wall
    summary.cpu_seconds = cpu_seconds() - cpu
    total = summary.total()
    with db:
        db.execute(
            "INSERT INTO ingests (started, host, wall_seconds, cpu_seconds, bytes_read, "
            "rows, skipped) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                summary.started,
                host,
                summary.wall_seconds,
                summary.cpu_seconds,
                total.bytes_read,
                total.rows,
                total.skipped,
            ),
        )
    return summary


def run(
    store_path: Path,
    lock_path: Path,
    sources: Sequence[Source],
    host: str,
    rebuild: bool = False,
) -> Summary:
    """One ingest under the lock: a second one waits for the first and then finds nothing
    new. `rebuild` recreates the store from the sources first."""
    with store.locked(lock_path):
        if rebuild:
            store.remove(store_path)
        db = store.open_store(store_path)
        try:
            return ingest(db, sources, host)
        finally:
            db.close()


def last_ingest(db: sqlite3.Connection) -> datetime | None:
    found = db.execute(
        "SELECT started FROM ingests ORDER BY id DESC LIMIT 1"
    ).fetchone()
    return datetime.fromisoformat(found[0]) if found and found[0] else None


def stale(store_path: Path, max_age: float) -> bool:
    """Whether the store is missing or its last ingest began over `max_age` seconds ago."""
    if not store_path.exists():
        return True
    try:
        db = store.open_read_only(store_path)
        try:
            last = last_ingest(db)
        finally:
            db.close()
    except sqlite3.Error:
        return True
    return last is None or (datetime.now(timezone.utc) - last).total_seconds() > max_age


def describe(summary: Summary) -> list[str]:
    """The ingest's lines for a person or a model to read."""
    total = summary.total()
    lines = [
        f"ingest  read {total.bytes_read} bytes  rows {total.rows}  "
        f"skipped {total.skipped}  wall {summary.wall_seconds:.2f} s  "
        f"cpu {summary.cpu_seconds:.2f} s"
    ]
    for name, tally in summary.sources.items():
        line = (
            f"  {name}  read {tally.bytes_read} bytes  rows {tally.rows}  "
            f"skipped {tally.skipped}"
        )
        lines.append(f"{line}  error: {tally.error}" if tally.error else line)
    return lines
