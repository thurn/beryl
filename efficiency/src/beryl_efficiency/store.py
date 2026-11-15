"""The store: one SQLite file in the cache (docs/efficiency-dashboard.md, "The store").

Every table is keyed so that ingesting the same bytes twice changes nothing. The schema grows
by appending to `MIGRATIONS`, never by editing an entry: the store is the long-term record,
because providers delete old transcripts, so a schema change must not need a rebuild.
"""

from __future__ import annotations

import contextlib
import os
import sqlite3
import sys
import time
from collections.abc import Iterator
from pathlib import Path

# Each entry moves the schema up one version (`PRAGMA user_version`). The tables are the
# design's; the columns of those no source fills yet are a first draft that the beads which
# fill them may change with a new entry.
MIGRATIONS: tuple[str, ...] = (
    """
    -- One row per model request.
    CREATE TABLE requests (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        message TEXT NOT NULL,
        time TEXT NOT NULL,
        model TEXT,
        input_tokens INTEGER,
        cache_write_tokens INTEGER,
        cache_read_tokens INTEGER,
        output_tokens INTEGER,
        dollars REAL,
        context_tokens INTEGER,
        prefix_tokens INTEGER,
        new_tokens INTEGER,
        bead TEXT,
        PRIMARY KEY (host, provider, transcript, message)
    );
    -- One row per tool call. `command` holds at most its first 300 characters.
    CREATE TABLE tool_calls (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        call TEXT NOT NULL,
        start TEXT NOT NULL,
        end TEXT,
        tool TEXT,
        kind TEXT,
        command TEXT,
        result_bytes INTEGER,
        added_tokens INTEGER,
        carrying_dollars REAL,
        bead TEXT,
        PRIMARY KEY (host, provider, transcript, call)
    );
    -- One row per bead window: a stretch of a session spent on one bead.
    CREATE TABLE windows (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        session TEXT NOT NULL,
        transcript TEXT NOT NULL,
        start TEXT NOT NULL,
        end TEXT,
        bead TEXT,
        PRIMARY KEY (host, provider, transcript, start)
    );
    -- The latest state of each bead. `enact` holds the bead's `enact_*` metadata as JSON.
    CREATE TABLE beads (
        id TEXT PRIMARY KEY,
        title TEXT,
        parent TEXT,
        issue_type TEXT,
        status TEXT,
        priority INTEGER,
        created_at TEXT,
        started_at TEXT,
        closed_at TEXT,
        enact TEXT NOT NULL DEFAULT '{}'
    );
    CREATE INDEX beads_parent ON beads (parent);
    CREATE TABLE labels (
        bead TEXT NOT NULL,
        label TEXT NOT NULL,
        PRIMARY KEY (bead, label)
    );
    CREATE INDEX labels_label ON labels (label);
    -- Each commit on staging. `paths` is a JSON list.
    CREATE TABLE commits (
        sha TEXT PRIMARY KEY,
        time TEXT NOT NULL,
        subject TEXT,
        paths TEXT NOT NULL DEFAULT '[]',
        bead TEXT
    );
    -- One row per check per day per script and host.
    CREATE TABLE checks (
        day TEXT NOT NULL,
        host TEXT NOT NULL,
        script TEXT NOT NULL,
        lane TEXT NOT NULL,
        "check" TEXT NOT NULL,
        runs INTEGER NOT NULL DEFAULT 0,
        fails INTEGER NOT NULL DEFAULT 0,
        skips INTEGER NOT NULL DEFAULT 0,
        seconds REAL NOT NULL DEFAULT 0,
        cpu_seconds REAL NOT NULL DEFAULT 0,
        PRIMARY KEY (day, host, script, lane, "check")
    );
    -- One row per sampler minute per owner, kind of work and worktree.
    CREATE TABLE cpu (
        host TEXT NOT NULL,
        time TEXT NOT NULL,
        owner TEXT NOT NULL,
        kind TEXT NOT NULL,
        worktree TEXT NOT NULL,
        seconds REAL NOT NULL,
        cpu_seconds REAL NOT NULL,
        PRIMARY KEY (host, time, owner, kind, worktree)
    );
    -- One row per CPU lease from the token pool.
    CREATE TABLE leases (
        host TEXT NOT NULL,
        start TEXT NOT NULL,
        owner TEXT NOT NULL,
        end TEXT,
        worktree TEXT,
        tokens INTEGER,
        waited_seconds REAL,
        PRIMARY KEY (host, start, owner)
    );
    -- For each source file (or command), how far ingest has read: the byte offset, a
    -- fingerprint of its first bytes, and how many malformed lines it skipped in all.
    CREATE TABLE sources (
        source TEXT NOT NULL,
        path TEXT NOT NULL,
        offset INTEGER NOT NULL DEFAULT 0,
        fingerprint TEXT NOT NULL DEFAULT '',
        skipped INTEGER NOT NULL DEFAULT 0,
        updated TEXT,
        PRIMARY KEY (source, path)
    );
    -- One row per ingest, with its own cost: the tool measures itself.
    CREATE TABLE ingests (
        id INTEGER PRIMARY KEY,
        started TEXT NOT NULL,
        host TEXT NOT NULL,
        wall_seconds REAL NOT NULL,
        cpu_seconds REAL NOT NULL,
        bytes_read INTEGER NOT NULL,
        rows INTEGER NOT NULL,
        skipped INTEGER NOT NULL
    );
    """,
)


def version() -> int:
    return len(MIGRATIONS)


def migrate(db: sqlite3.Connection) -> None:
    """Brings `db` up to the newest schema, one migration per transaction."""
    have = int(db.execute("PRAGMA user_version").fetchone()[0])
    if have > version():
        raise RuntimeError(
            f"the store has schema {have}, newer than this code's {version()}"
        )
    for number, script in enumerate(MIGRATIONS[have:], start=have + 1):
        db.executescript(f"BEGIN; {script}; PRAGMA user_version = {number}; COMMIT;")


def open_store(path: Path) -> sqlite3.Connection:
    """The store at `path` for writing, made and migrated if needed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(path)
    db.execute("PRAGMA journal_mode = WAL")
    migrate(db)
    return db


def open_read_only(path: Path) -> sqlite3.Connection:
    """The store at `path`, for queries: SQLite refuses every write through it."""
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    db.execute("PRAGMA query_only = ON")
    return db


def tables(db: sqlite3.Connection) -> list[str]:
    return [
        str(r[0])
        for r in db.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name"
        )
    ]


@contextlib.contextmanager
def locked(path: Path) -> Iterator[None]:
    """Holds the ingest lock at `path`, waiting for any ingest that holds it. The operating
    system releases it if the process dies."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a+b") as handle:
        if sys.platform == "win32":
            import msvcrt

            handle.seek(0)
            while True:
                try:
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    time.sleep(0.2)
            try:
                yield
            finally:
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl

            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def remove(path: Path) -> None:
    """Deletes the store at `path` with its write-ahead log."""
    for suffix in ("", "-wal", "-shm"):
        with contextlib.suppress(FileNotFoundError):
            os.remove(f"{path}{suffix}")
