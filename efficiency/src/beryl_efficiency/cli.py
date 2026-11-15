"""`just efficiency <query>`: the dashboard's command (docs/efficiency-dashboard.md, "The
command"). Each query prints a compact text table, or with `--json` the result the page will
render. A query first ingests when the store's last ingest is over a minute old, unless
`--no-ingest` is given."""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections.abc import Callable, Sequence
from pathlib import Path

from . import bead, claude, ingest, paths, store
from .beads import Beads, main_checkout
from .render import Result, table, text

# How old the store may be before a query ingests first, in seconds.
MAX_AGE = 60.0


def sources() -> list[ingest.Source]:
    """Every source, in the order ingest reads them: beads and session records first, since
    deriving windows from transcripts reads both."""
    checkout = main_checkout(Path.cwd())
    host = paths.host()
    return [
        Beads.here(checkout),
        claude.Sessions(paths.claude_sessions(), checkout, host),
        claude.Transcripts(paths.claude_projects(), checkout, host),
    ]


def update(rebuild: bool = False) -> ingest.Summary:
    if rebuild:
        print(
            "efficiency: rebuilding the store from the sources; rows from transcripts "
            "older than the providers' retention are lost",
            file=sys.stderr,
        )
    return ingest.run(paths.store(), paths.lock(), sources(), paths.host(), rebuild)


def fresh(args: argparse.Namespace) -> None:
    """Ingests first when the store is older than `MAX_AGE`, unless `--no-ingest`."""
    if not args.no_ingest and ingest.stale(paths.store(), MAX_AGE):
        update()


def reader() -> sqlite3.Connection:
    path = paths.store()
    if not path.exists():
        store.open_store(path).close()  # An empty store answers with empty results.
    return store.open_read_only(path)


def run_ingest(args: argparse.Namespace) -> Result:
    summary = update(rebuild=bool(args.rebuild))
    total = summary.total()
    return Result(
        "ingest",
        {
            "started": summary.started,
            "wall_seconds": round(summary.wall_seconds, 3),
            "cpu_seconds": round(summary.cpu_seconds, 3),
            "bytes_read": total.bytes_read,
            "rows": total.rows,
            "skipped": total.skipped,
            "sources": {
                name: {
                    "bytes_read": t.bytes_read,
                    "rows": t.rows,
                    "skipped": t.skipped,
                    "error": t.error,
                }
                for name, t in summary.sources.items()
            },
        },
        ingest.describe(summary),
        ["efficiency coverage"],
    )


def run_sql(args: argparse.Namespace) -> Result:
    fresh(args)
    db = reader()
    try:
        cursor = db.execute(str(args.query))
        headers = [str(d[0]) for d in cursor.description or []]
        rows = [list(r) for r in cursor.fetchall()]
    finally:
        db.close()
    return Result(
        "sql",
        {"columns": headers, "rows": rows},
        table(headers, rows),
        [],
    )


def unread(db: sqlite3.Connection, found: Sequence[ingest.Source]) -> dict[str, int]:
    """Files of lines that ingest has not read to their end: how many were never read, how
    many were read in part, and the bytes still to read. A file's last line still being
    written counts as unread."""
    counts = {"never_read": 0, "partly_read": 0, "bytes": 0}
    for source in found:
        if not isinstance(source, ingest.LineSource):
            continue
        for path in source.files():
            try:
                size = path.stat().st_size
            except OSError:
                continue
            offset, recorded = ingest.position(db, source.name, str(path))
            if not recorded:
                counts["never_read"] += 1
            elif size > offset:
                counts["partly_read"] += 1
            counts["bytes"] += max(size - offset, 0)
    return counts


def gaps(db: sqlite3.Connection) -> dict[str, object]:
    """What attribution could not place: top-level transcripts with no bead window (by the
    kind of their one window), closes with no open window, and beads that ran on a host the
    store has no data from, by host."""
    without = {
        str(r[0]): int(r[1])
        for r in db.execute(
            """SELECT w.kind, count(DISTINCT w.transcript) FROM windows AS w
            WHERE NOT EXISTS (SELECT 1 FROM windows AS b WHERE b.host = w.host
                AND b.provider = w.provider AND b.transcript = w.transcript
                AND b.kind = 'bead')
            GROUP BY w.kind ORDER BY w.kind"""
        )
    }
    stray = int(
        db.execute("SELECT coalesce(sum(stray_closes), 0) FROM transcripts").fetchone()[
            0
        ]
    )
    elsewhere = {
        str(r[0]): int(r[1])
        for r in db.execute("""SELECT h, count(*) FROM (SELECT coalesce(
                json_extract(enact, '$.enact_spawn_host'),
                json_extract(enact, '$.enact_worktree_host')) AS h FROM beads)
            WHERE h IS NOT NULL AND h NOT IN (SELECT DISTINCT host FROM transcripts)
            GROUP BY h ORDER BY h""")
    }
    return {
        "sessions_without_window": without,
        "stray_closes": stray,
        "beads_no_data": elsewhere,
    }


def coverage(db: sqlite3.Connection, pending: dict[str, int] | None = None) -> Result:
    """What the store holds and what it is missing: rows per table, skipped lines per source,
    what attribution could not place, files not yet read, and the cost of the ingests, all of
    them and the recent ones."""
    counts = {
        name: int(db.execute(f'SELECT count(*) FROM "{name}"').fetchone()[0])
        for name in store.tables(db)
    }
    skipped = {
        str(r[0]): int(r[1])
        for r in db.execute(
            "SELECT source, sum(skipped) FROM sources GROUP BY source ORDER BY source"
        )
    }
    recent = [
        [str(r[0]), str(r[1]), round(float(r[2]), 3), round(float(r[3]), 3)]
        + [int(r[4]), int(r[5]), int(r[6])]
        for r in db.execute(
            "SELECT started, host, wall_seconds, cpu_seconds, bytes_read, rows, skipped "
            "FROM ingests ORDER BY id DESC LIMIT 10"
        )
    ]
    found = db.execute(
        "SELECT count(*), coalesce(sum(wall_seconds), 0), coalesce(sum(cpu_seconds), 0), "
        "min(started) FROM ingests"
    ).fetchone()
    count, wall, cpu = (
        int(found[0]),
        round(float(found[1]), 3),
        round(float(found[2]), 3),
    )
    since = str(found[3]) if found[3] else None
    columns = ["started", "host", "wall_seconds", "cpu_seconds", "bytes_read", "rows"]
    columns.append("skipped")
    missing = gaps(db)
    without = missing["sessions_without_window"]
    elsewhere = missing["beads_no_data"]
    assert isinstance(without, dict) and isinstance(elsewhere, dict)
    lines = [
        "rows      " + " | ".join(f"{k} {v}" for k, v in counts.items()),
        "skipped   "
        + (" | ".join(f"{k} {v}" for k, v in skipped.items()) or "no source read yet"),
        "no bead   sessions with no bead window: "
        + (", ".join(f"{k} {v}" for k, v in without.items()) or "none")
        + f" | closes with no open window {missing['stray_closes']}",
        "no data   beads that ran on hosts with no data: "
        + (", ".join(f"{k} {v}" for k, v in elsewhere.items()) or "none"),
    ]
    if pending is not None:
        lines.append(
            f"unread    files never read {pending['never_read']}, read in part "
            f"{pending['partly_read']}, bytes to read {pending['bytes']}"
        )
    lines.append(
        f"ingests   {count} since {since or '-'}: wall {wall:.2f} s  cpu {cpu:.2f} s"
    )
    if recent:
        lines += ["", "recent ingests", *table(columns, recent)]
    return Result(
        "coverage",
        {
            "tables": counts,
            "skipped": skipped,
            "ingest_cost": {
                "count": count,
                "wall_seconds": wall,
                "cpu_seconds": cpu,
                "since": since,
            },
            "ingests": [dict(zip(columns, r)) for r in recent],
            **missing,
            "unread": pending,
        },
        lines,
        ["efficiency sql 'select * from sources'"],
    )


def run_coverage(args: argparse.Namespace) -> Result:
    fresh(args)
    db = reader()
    try:
        return coverage(db, unread(db, sources()))
    finally:
        db.close()


def run_bead(args: argparse.Namespace) -> Result:
    fresh(args)
    db = reader()
    try:
        return bead.report(db, str(args.bead))
    finally:
        db.close()


QUERIES: dict[str, tuple[str, Callable[[argparse.Namespace], Result]]] = {
    "ingest": ("update the store from the sources", run_ingest),
    "sql": ("run a read-only SQL query against the store", run_sql),
    "coverage": (
        "what the store holds and misses, and ingest's own cost",
        run_coverage,
    ),
    "bead": ("one bead's requests, tokens, minutes and sessions", run_bead),
}


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(
        prog="just efficiency",
        description="Where agent time, CPU and tokens go, per bead.",
    )
    sub = top.add_subparsers(dest="command", metavar="<query>")
    for name, (summary, _) in QUERIES.items():
        p = sub.add_parser(name, help=summary, description=summary)
        p.add_argument("--json", action="store_true", help="print the result as JSON")
        if name == "ingest":
            p.add_argument(
                "--rebuild",
                action="store_true",
                help="recreate the store from the sources",
            )
        else:
            p.add_argument(
                "--no-ingest",
                action="store_true",
                help="answer from the store as it is",
            )
        if name == "sql":
            p.add_argument("query", metavar="'<query>'", help="a read-only SQL query")
        if name == "bead":
            p.add_argument("bead", metavar="<id>", help="the bead's id")
    return top


def main(argv: Sequence[str] | None = None) -> int:
    top = parser()
    args = top.parse_args(argv)
    if args.command is None:
        print("just efficiency <query>")
        for name, (summary, _) in QUERIES.items():
            print(f"  {name:<10} {summary}")
        return 0
    try:
        result = QUERIES[str(args.command)][1](args)
    except sqlite3.Error as e:
        print(f"efficiency: {e}", file=sys.stderr)
        return 1
    print(json.dumps(result.json(), indent=1) if args.json else text(result))
    return 0
