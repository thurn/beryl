"""Moving rows between machines (docs/efficiency-dashboard.md, "Other machines").

`export` writes this host's rows from every table that carries a host as one JSON-lines file,
gzip-compressed unless its name ends in `.jsonl`: a header line, then one `{"table": ..., "row": {...}}` line per row. `import`
adds the rows of such a file to this store. Every table is keyed by host and source, so a row
already present is left alone and importing a file twice, or two overlapping exports, changes
nothing after the first. The aggregated tables `checks` and `lane_runs` are the exception: a
later export holds a fuller total for the same key, so its row replaces the stored one.

An export holds commands and paths, as the store does. It is copied between a person's own
machines and never committed or published.
"""

from __future__ import annotations

import gzip
import json
import sqlite3
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from pathlib import Path

from . import store

FORMAT = 1
# The column that dates each table's rows, for `--since`. A table not listed (`sessions`,
# which has no time of its own) is exported whole.
TIME_COLUMNS: dict[str, str] = {
    "requests": "time",
    "tool_calls": "start",
    "windows": "start",
    "transcripts": "first",
    "enact_events": "time",
    "tool_results": "end",
    "prompts": "time",
    "window_time": "start",
    "window_dollars": "start",
    "codex_marks": "time",
    "cpu": "time",
    "cpu_minutes": "time",
    "leases": "time",
    "checks": "first",
    "lane_runs": "start",
    "verdicts": "time",
    "ignite_records": "at",
}
# Tables that hold per-host bookkeeping rather than data, and `plan_usage`, which the hub's
# machine reads from the hub itself: not exported.
SKIPPED = {"ingests", "plan_usage"}
# Tables whose rows are totals that grow, so a later row replaces an earlier one.
REPLACED = {"checks", "lane_runs"}


class ExchangeError(Exception):
    """A file that cannot be imported."""


def host_tables(db: sqlite3.Connection) -> dict[str, list[str]]:
    """Every table that has a `host` column, with its columns in order."""
    found: dict[str, list[str]] = {}
    for name in store.tables(db):
        if name in SKIPPED:
            continue
        columns = [str(r[1]) for r in db.execute(f'PRAGMA table_info("{name}")')]
        if "host" in columns:
            found[name] = columns
    return found


def export(
    db: sqlite3.Connection, host: str, since: str | None, out: Path
) -> dict[str, int]:
    """Writes `host`'s rows dated at or after `since` (a UTC stamp, or none for all) to `out`
    and returns the row count per table. The file is written to a temporary name and renamed
    so that a failed export leaves no partial file."""
    counts: dict[str, int] = {}
    temporary = out.with_name(out.name + ".part")
    out.parent.mkdir(parents=True, exist_ok=True)
    plain = out.suffix == ".jsonl"
    with (
        open(temporary, "w", encoding="utf-8")
        if plain
        else gzip.open(temporary, "wt", encoding="utf-8")
    ) as handle:
        header = {
            "export": FORMAT,
            "host": host,
            "schema": store.version(),
            "since": since,
        }
        handle.write(json.dumps(header) + "\n")
        for name, columns in host_tables(db).items():
            where, args = "host = ?", [host]
            column = TIME_COLUMNS.get(name)
            if since is not None and column is not None:
                where += f' AND "{column}" >= ?'
                # `checks` is dated by its day's first line; its `day` is a local date, so
                # the stamp is compared as text, which orders the same way.
                args.append(since)
            quoted = ", ".join(f'"{c}"' for c in columns)
            cursor = db.execute(f'SELECT {quoted} FROM "{name}" WHERE {where}', args)
            count = 0
            for values in cursor:
                row = dict(zip(columns, values))
                handle.write(json.dumps({"table": name, "row": row}) + "\n")
                count += 1
            counts[name] = count
    temporary.replace(out)
    return counts


@dataclass
class Imported:
    host: str = ""
    added: dict[str, int] = field(default_factory=dict)
    seen: dict[str, int] = field(default_factory=dict)


def import_file(db: sqlite3.Connection, path: Path, host: str) -> Imported:
    """Adds the rows of the export at `path` (gzip-compressed or plain) to `db`, in one
    transaction. Rows of this machine's own `host` are refused: ingest is the authority on
    them."""
    try:
        with open(path, "rb") as probe:
            compressed = probe.read(2) == b"\x1f\x8b"
        handle = (
            gzip.open(path, "rt", encoding="utf-8")
            if compressed
            else open(path, encoding="utf-8")
        )
    except OSError as e:
        raise ExchangeError(f"cannot read {path}: {e}") from e
    with handle:
        try:
            db.execute("BEGIN")
            found = import_lines(db, handle, host)
            db.execute("COMMIT")
        except ExchangeError:
            db.execute("ROLLBACK")
            raise
        except (OSError, EOFError) as e:
            db.execute("ROLLBACK")
            raise ExchangeError(f"cannot read {path}: {e}") from e
    return found


def import_lines(
    db: sqlite3.Connection,
    lines: Iterable[str],
    host: str,
    rename: Callable[[str], str] | None = None,
) -> Imported:
    """Adds the rows of an export's lines, header first, to `db` inside the caller's
    transaction. `rename` turns the export's host into the name its rows are stored under
    (Ignite names a machine by the first host seen for its ID); a file whose host, so named,
    is this machine's `host` is refused. Table and column names come from the file, so each
    is checked against this store's schema before it is used."""
    found = Imported()
    iterator = iter(lines)
    try:
        header = json.loads(next(iterator))
    except (StopIteration, ValueError):
        raise ExchangeError("not an efficiency export: the first line is not JSON")
    if not isinstance(header, dict) or header.get("export") != FORMAT:
        raise ExchangeError("not an efficiency export of a format this code reads")
    schema = header.get("schema")
    if not isinstance(schema, int) or schema > store.version():
        raise ExchangeError(
            f"the export has schema {schema}, newer than this code's "
            f"{store.version()}: update this machine first"
        )
    source = str(header.get("host") or "")
    if not source:
        raise ExchangeError("the export names no host")
    named = rename(source) if rename else source
    if named == host:
        raise ExchangeError(
            f"the export is from this host ({host}); ingest is the source of its rows"
        )
    found.host = named
    tables = host_tables(db)
    try:
        for line in iterator:
            if not line.strip():
                continue
            record = json.loads(line)
            name, row = record["table"], record["row"]
            if name not in tables or not isinstance(row, dict):
                raise ExchangeError(f"unknown table {name!r} in the export")
            if row.get("host") != source:
                raise ExchangeError(f"a row of {name} is not from {source}")
            row = {**row, "host": named}
            columns = [str(c) for c in row]
            unknown = [c for c in columns if c not in tables[name]]
            if unknown:
                raise ExchangeError(f"{name} has no column {unknown[0]!r}")
            found.seen[name] = found.seen.get(name, 0) + 1
            same = " AND ".join(f'"{c}" IS ?' for c in columns)
            values = [row[c] for c in columns]
            if db.execute(f'SELECT 1 FROM "{name}" WHERE {same}', values).fetchone():
                continue
            verb = "REPLACE" if name in REPLACED else "IGNORE"
            quoted = ", ".join(f'"{c}"' for c in columns)
            marks = ", ".join("?" for _ in columns)
            cursor = db.execute(
                f'INSERT OR {verb} INTO "{name}" ({quoted}) VALUES ({marks})', values
            )
            if cursor.rowcount > 0:
                found.added[name] = found.added.get(name, 0) + 1
    except (ValueError, KeyError, TypeError) as e:
        raise ExchangeError(f"damaged export: {e}") from e
    return found
