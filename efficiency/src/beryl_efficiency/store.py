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
    """
    -- A window's `kind` is `bead` (a stretch spent on the bead `bead`), `orchestration` (a
    -- whole orchestrator session, with its epic as `bead`) or `none` (no bead; `label` holds
    -- the worktree the session created, if any). A top-level transcript's windows cover it
    -- from its first record to its last with no gap and no overlap.
    ALTER TABLE windows ADD COLUMN kind TEXT NOT NULL DEFAULT 'bead';
    ALTER TABLE windows ADD COLUMN label TEXT;
    CREATE INDEX windows_bead ON windows (bead);
    CREATE INDEX requests_bead ON requests (bead);
    CREATE INDEX tool_calls_bead ON tool_calls (bead);
    -- Which session each transcript belongs to: a desktop session (`local_<uuid>`) has one
    -- transcript at a time, and a new one each time it is cleared. `left_at` is when the
    -- session left an earlier transcript.
    CREATE TABLE sessions (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        session TEXT NOT NULL,
        title TEXT,
        model TEXT,
        spawned_from TEXT,
        left_at TEXT,
        PRIMARY KEY (host, provider, transcript)
    );
    -- One row per transcript read: its session (the transcript itself when no record names
    -- it), the transcript that started it for a subagent, its first and last record, and how
    -- many `enact close` commands it ran with no open window for their bead.
    CREATE TABLE transcripts (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        parent TEXT,
        session TEXT NOT NULL,
        first TEXT,
        last TEXT,
        stray_closes INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (host, provider, transcript)
    );
    -- Enact commands a session ran, from which windows are found. `verb` is `open`
    -- (`enact start` or `enact claim`), `close`, `next` (the bead `enact close --continue`
    -- claimed, at its result) or `worktree` (`enact worktree create <name>`).
    CREATE TABLE enact_events (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        call TEXT NOT NULL,
        verb TEXT NOT NULL,
        bead TEXT NOT NULL,
        time TEXT NOT NULL,
        PRIMARY KEY (host, provider, transcript, call, verb, bead)
    );
    -- When each tool call ended and its output's size. A result is on a later line than its
    -- call, so ingest keeps it here and copies it into `tool_calls`.
    CREATE TABLE tool_results (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        call TEXT NOT NULL,
        end TEXT NOT NULL,
        result_bytes INTEGER,
        PRIMARY KEY (host, provider, transcript, call)
    );
    """,
    """
    -- Why a request stopped (`end_turn`, `tool_use`, ...): a turn ends at a request that did
    -- not stop for a tool.
    ALTER TABLE requests ADD COLUMN stop_reason TEXT;
    -- `chained` is 1 for a shell command that chains two or more heavy commands. Of a call's
    -- seconds, `compile_seconds` were cargo building (from its `Finished ... in` lines) and
    -- `queued_seconds` were spent waiting for a CPU lease.
    ALTER TABLE tool_calls ADD COLUMN chained INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE tool_calls ADD COLUMN compile_seconds REAL;
    ALTER TABLE tool_calls ADD COLUMN queued_seconds REAL NOT NULL DEFAULT 0;
    ALTER TABLE tool_results ADD COLUMN compile_seconds REAL;
    -- A message that started a turn, other than a tool result. `kind` is `person` (typed by a
    -- person, or an interrupt), `background` (a background command or another session
    -- finished; `detail` is the command's tool call id, or `session`) or `idle` (anything
    -- else, such as a scheduled wake-up).
    CREATE TABLE prompts (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        time TEXT NOT NULL,
        kind TEXT NOT NULL,
        detail TEXT,
        PRIMARY KEY (host, provider, transcript, time)
    );
    -- Each window's wall-clock seconds split by category (`person`, `queued`, `tool`,
    -- `model`, `background`, `idle`), kind of command and part (`compile`, `run`, or empty).
    -- A window's rows sum to its length. `start` is the window's start.
    CREATE TABLE window_time (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        start TEXT NOT NULL,
        category TEXT NOT NULL,
        kind TEXT NOT NULL DEFAULT '',
        part TEXT NOT NULL DEFAULT '',
        seconds REAL NOT NULL,
        PRIMARY KEY (host, provider, transcript, start, category, kind, part)
    );
    -- Values the store keeps about itself, such as the digest of the command kinds table
    -- the stored kinds came from.
    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
    -- Transcripts are read again from the start, for the stop reasons, prompts and compile
    -- times that earlier ingests did not keep.
    DELETE FROM sources WHERE source = 'claude';
    """,
    """
    -- The part of `cache_write_tokens` written to the 1-hour cache, which costs more than
    -- the 5-minute one, and each request's dollars by class (null when its model has no
    -- price).
    ALTER TABLE requests ADD COLUMN cache_write_1h_tokens INTEGER;
    ALTER TABLE requests ADD COLUMN input_dollars REAL;
    ALTER TABLE requests ADD COLUMN cache_write_dollars REAL;
    ALTER TABLE requests ADD COLUMN cache_read_dollars REAL;
    ALTER TABLE requests ADD COLUMN output_dollars REAL;
    -- Each window's tokens and dollars split three ways, each summing to the window's
    -- dollars: `split` is `class` (input, cache_write, cache_read, output), `source` (fixed
    -- prefix, `new: <who>`, `carried: <who>`, output) or `purpose` (wake-up, working). A
    -- row of split `unpriced` names a model with no price whose requests the dollars leave
    -- out, so the window's dollars are a lower bound. `start` is the window's start.
    CREATE TABLE window_dollars (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        start TEXT NOT NULL,
        split TEXT NOT NULL,
        name TEXT NOT NULL,
        tokens REAL NOT NULL,
        dollars REAL NOT NULL,
        PRIMARY KEY (host, provider, transcript, start, split, name)
    );
    -- Transcripts are read again from the start for the 1-hour cache writes.
    DELETE FROM sources WHERE source = 'claude';
    """,
    """
    -- `window_dollars` gains the split `model`: each window's priced dollars by the model
    -- that made the requests. Token accounting writes it when it next accounts every
    -- transcript, which a change to its version makes happen.
    SELECT 1;
    """,
    """
    -- An ingest stopped after committing a transcript's rows but before deriving its
    -- windows left those rows with no bead, and later ingests saw the file as read.
    -- Ingest now marks such files in `settings` until their windows are derived; reading
    -- every transcript again repairs the rows left before that.
    DELETE FROM sources WHERE source = 'claude';
    """,
    """
    -- Facts in a Codex log that belong to a request but are on other lines: `kind` is `model`
    -- (the model a turn used, in `value`) or `turn_end` (a turn finished or was aborted).
    -- Ingest copies them onto the transcript's requests as `model` and `stop_reason`.
    CREATE TABLE codex_marks (
        host TEXT NOT NULL,
        provider TEXT NOT NULL,
        transcript TEXT NOT NULL,
        time TEXT NOT NULL,
        kind TEXT NOT NULL,
        value TEXT NOT NULL DEFAULT '',
        PRIMARY KEY (host, provider, transcript, time, kind)
    );
    """,
    """
    -- CPU accounting. `cpu` holds the sampler's rows, one per minute per owner, kind of work
    -- and worktree (empty for none); `cpu_minutes` holds each minute's totals: its length,
    -- the machine's cores, the CPU seconds of every owner and the busiest moment.
    CREATE INDEX cpu_worktree ON cpu (worktree, time);
    CREATE TABLE cpu_minutes (
        host TEXT NOT NULL,
        time TEXT NOT NULL,
        seconds REAL NOT NULL,
        cores INTEGER NOT NULL,
        cpu_seconds REAL NOT NULL,
        peak_cores REAL,
        PRIMARY KEY (host, time)
    );
    -- One row per CPU lease, from the token pool's log: `time` is its release, and the
    -- command waited from `wait_start` to `wait_end` for its tokens. `transcript` and `call`
    -- name the tool call that was running in that worktree while it waited; `call` is empty
    -- when none was found and null until ingest has looked.
    DROP TABLE leases;
    CREATE TABLE leases (
        host TEXT NOT NULL,
        time TEXT NOT NULL,
        class TEXT NOT NULL,
        worktree TEXT NOT NULL,
        command TEXT NOT NULL,
        cores INTEGER NOT NULL,
        wanted INTEGER NOT NULL,
        waited_seconds REAL NOT NULL,
        held_seconds REAL NOT NULL,
        wait_start TEXT NOT NULL,
        wait_end TEXT NOT NULL,
        provider TEXT,
        transcript TEXT,
        call TEXT,
        PRIMARY KEY (host, time, class, worktree, command, waited_seconds, held_seconds)
    );
    CREATE INDEX leases_call ON leases (host, provider, transcript, call);
    CREATE INDEX tool_calls_start ON tool_calls (host, start);
    -- A tool call's queued seconds are worked out from the leases joined to it, which
    -- survive the call's row being read again.
    ALTER TABLE tool_calls DROP COLUMN queued_seconds;
    """,
    """
    -- CI accounting, from the run log and the verdicts on the checks' beads. `checks` adds
    -- up the run log's lines: one row per check per local day per host and script, with
    -- `first` its earliest line's time. `runs` counts the lines that ran (passed, failed or
    -- pending), `seconds` their wall seconds, `cpu_seconds` their measured CPU and
    -- `measured_seconds` the wall seconds of the lines that measured it, so that the CPU of
    -- the rest can be estimated.
    DROP TABLE checks;
    CREATE TABLE checks (
        day TEXT NOT NULL,
        host TEXT NOT NULL,
        script TEXT NOT NULL,
        lane TEXT NOT NULL,
        "check" TEXT NOT NULL,
        first TEXT NOT NULL,
        runs INTEGER NOT NULL,
        passes INTEGER NOT NULL,
        fails INTEGER NOT NULL,
        skips INTEGER NOT NULL,
        seconds REAL NOT NULL,
        cpu_seconds REAL NOT NULL,
        measured_seconds REAL NOT NULL,
        PRIMARY KEY (day, host, script, lane, "check")
    );
    -- One row per run of a check lane: the run log's lines of one script, worktree, commit,
    -- tree and lane added up, from the earliest check's start to the latest one's end.
    CREATE TABLE lane_runs (
        host TEXT NOT NULL,
        script TEXT NOT NULL,
        worktree TEXT NOT NULL,
        "commit" TEXT NOT NULL,
        tree TEXT NOT NULL,
        lane TEXT NOT NULL,
        start TEXT NOT NULL,
        end TEXT NOT NULL,
        checks INTEGER NOT NULL,
        fails INTEGER NOT NULL,
        seconds REAL NOT NULL,
        cpu_seconds REAL NOT NULL,
        measured_seconds REAL NOT NULL,
        PRIMARY KEY (host, script, worktree, "commit", tree, lane)
    );
    CREATE INDEX lane_runs_start ON lane_runs (script, start);
    -- One row per verdict on a check's failure, keyed by its comment on the check's bead;
    -- `time` is the failure's.
    CREATE TABLE verdicts (
        comment TEXT PRIMARY KEY,
        "check" TEXT NOT NULL,
        time TEXT NOT NULL,
        host TEXT NOT NULL,
        "commit" TEXT NOT NULL,
        script TEXT NOT NULL,
        lane TEXT NOT NULL,
        verdict TEXT NOT NULL,
        steward TEXT
    );
    """,
    """
    -- Commits on staging and master, from `git log`. `parents`, `paths` (every file the
    -- commit changed, both names of a rename) and `added` (files it created) are JSON
    -- lists. A commit's bead is read from the beads at query time.
    DROP TABLE commits;
    CREATE TABLE commits (
        sha TEXT PRIMARY KEY,
        time TEXT NOT NULL,
        subject TEXT,
        parents TEXT NOT NULL DEFAULT '[]',
        paths TEXT NOT NULL DEFAULT '[]',
        added TEXT NOT NULL DEFAULT '[]'
    );
    -- Each time a branch moved, from its reflog, kept after git expires the entry.
    CREATE TABLE ref_moves (
        ref TEXT NOT NULL,
        time TEXT NOT NULL,
        sha TEXT NOT NULL,
        PRIMARY KEY (ref, time, sha)
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
