"""`just efficiency <query>`: the dashboard's command (docs/efficiency-dashboard.md, "The
command"). Each query prints a compact text table, or with `--json` the result the page will
render. A query first ingests when the store's last ingest is over a minute old, unless
`--no-ingest` is given."""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from datetime import datetime
from collections.abc import Callable, Sequence
from pathlib import Path

from . import (
    bead,
    cards,
    changes,
    ci,
    cilog,
    claude,
    commits,
    codex,
    cpu,
    exchange,
    facts,
    failures,
    findings,
    ingest,
    kinds,
    machine,
    metrics,
    paths,
    pushes,
    rollups,
    session,
    store,
)
from .beads import Beads, main_checkout
from .render import Result, table, text
from .scope import Scope, add_arguments, from_arguments, parse_since

# How old the store may be before a query ingests first, in seconds.
MAX_AGE = 60.0


def sources() -> list[ingest.Source]:
    """Every source, in the order ingest reads them: beads and session records first, since
    deriving windows from transcripts reads both, and the CPU token pool's leases after the
    transcripts, since they are joined to the transcripts' tool calls."""
    checkout = main_checkout(Path.cwd())
    host = paths.host()
    return [
        Beads.here(checkout),
        claude.Sessions(paths.claude_sessions(), checkout, host),
        claude.Transcripts(paths.claude_projects(), checkout, host),
        codex.Rollouts(paths.codex_sessions(), checkout, host),
        cpu.Samples(cpu.samples_path(paths.cache_root()), host),
        cpu.Leases(cpu.leases_path(paths.cache_root()), host),
        cilog.RunLog(cilog.runlog_path(paths.cache_root())),
        cilog.Verdicts.here(checkout),
        commits.Commits.here(checkout),
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


def run_export(args: argparse.Namespace) -> Result:
    fresh(args)
    host = paths.host()
    since = parse_since(str(args.since)).since
    today = datetime.now().strftime("%Y-%m-%d")
    out = Path(args.out) if args.out else paths.folder() / f"export-{host}-{today}.gz"
    db = reader()
    try:
        counts = exchange.export(db, host, since, out)
    finally:
        db.close()
    return Result(
        "export",
        {"host": host, "since": since, "file": str(out), "rows": counts},
        [
            f"wrote {sum(counts.values())} rows of {host} to {out}",
            "It holds commands and paths: copy it to your own machines only, never "
            "commit or publish it.",
        ],
        [f"efficiency import {out}"],
    )


def run_import(args: argparse.Namespace) -> Result:
    path = Path(args.file)
    db = store.open_store(paths.store())
    try:
        with store.locked(paths.lock()):
            try:
                found = exchange.import_file(db, path, paths.host())
            except exchange.ExchangeError as e:
                raise SystemExit(f"efficiency: {e}") from e
    finally:
        db.close()
    return Result(
        "import",
        {"host": found.host, "added": found.added, "seen": found.seen},
        [
            f"imported {sum(found.added.values())} new rows of {found.host} "
            f"({sum(found.seen.values())} in the file) from {path}"
        ],
        ["efficiency coverage"],
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
    # A bead ran with a provider (Claude unless the bead says otherwise): it has no data when
    # the store holds no transcript of that provider from its host.
    elsewhere = {
        str(r[0]): int(r[1])
        for r in db.execute(
            """SELECT CASE WHEN p = 'claude' THEN h ELSE h || ' (' || p || ')' END,
                count(*)
            FROM (SELECT coalesce(json_extract(enact, '$.enact_spawn_host'),
                    json_extract(enact, '$.enact_worktree_host')) AS h,
                    coalesce(json_extract(enact, '$.enact_spawn_provider'), 'claude') AS p
                FROM beads)
            WHERE h IS NOT NULL AND NOT EXISTS (SELECT 1 FROM transcripts AS t
                WHERE t.host = h AND t.provider = p)
            GROUP BY 1 ORDER BY 1"""
        )
    }
    return {
        "sessions_without_window": without,
        "stray_closes": stray,
        "beads_no_data": elsewhere,
    }


def unmatched(db: sqlite3.Connection, days: int = 7) -> dict[str, object]:
    """The share of the last `days` days' tool time whose command matched no kind, and the
    unmatched commands that took longest, so that the kinds table can be extended."""
    since = f"-{days} days"
    duration = "(julianday(end) - julianday(start)) * 86400"
    found = db.execute(
        f"""SELECT coalesce(sum({duration}), 0),
            coalesce(sum(CASE WHEN kind = ? THEN {duration} END), 0)
        FROM tool_calls WHERE end IS NOT NULL
            AND start >= strftime('%Y-%m-%dT%H:%M:%S', 'now', ?)""",
        (kinds.OTHER, since),
    ).fetchone()
    total, other = float(found[0]), float(found[1])
    commands = [
        {"command": str(r[0]), "calls": int(r[1]), "seconds": round(float(r[2]), 1)}
        for r in db.execute(
            f"""SELECT substr(command, 1, 100) AS c, count(*), sum({duration}) AS s
            FROM tool_calls WHERE end IS NOT NULL AND kind = ?
                AND start >= strftime('%Y-%m-%dT%H:%M:%S', 'now', ?)
            GROUP BY c ORDER BY s DESC LIMIT 10""",
            (kinds.OTHER, since),
        )
    ]
    return {
        "days": days,
        "tool_seconds": round(total, 1),
        "other_seconds": round(other, 1),
        "other_share": round(other / total, 4) if total else None,
        "commands": commands,
    }


# The tables whose newest row says how fresh a host's data is, and those that a host may
# legitimately lack (Windows has no CPU sampler or lease log).
FRESHNESS: dict[str, str] = {
    name: column
    for name, column in exchange.TIME_COLUMNS.items()
    if name in ("requests", "tool_calls", "windows", "cpu_minutes", "lane_runs")
}
OPTIONAL = ("cpu_minutes", "leases")


def hosts(db: sqlite3.Connection) -> dict[str, dict[str, object]]:
    """Per host, the date of its newest row and the CPU tables that hold nothing for it, so
    that a stale import, or a host with no sampler, is visible."""
    found: dict[str, dict[str, object]] = {}
    for name, column in FRESHNESS.items():
        for host, newest in db.execute(
            f'SELECT host, max("{column}") FROM "{name}" GROUP BY host'
        ):
            entry = found.setdefault(str(host), {"newest": None})
            if newest and (
                entry["newest"] is None or str(newest) > str(entry["newest"])
            ):
                entry["newest"] = str(newest)
    for entry in found.values():
        entry["newest"] = str(entry["newest"])[:10] if entry["newest"] else None
    for host, entry in found.items():
        entry["no_rows"] = [
            name
            for name in OPTIONAL
            if not db.execute(
                f'SELECT 1 FROM "{name}" WHERE host = ? LIMIT 1', (host,)
            ).fetchone()
        ]
    return dict(sorted(found.items()))


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
    per_host = hosts(db)
    lines.append(
        "hosts     newest row: "
        + (
            ", ".join(
                f"{h} {e['newest'] or '-'}"
                + (f" (no {', '.join(e['no_rows'])})" if e["no_rows"] else "")  # type: ignore[attr-defined]
                for h, e in per_host.items()
            )
            or "none"
        )
    )
    if pending is not None:
        lines.append(
            f"unread    files never read {pending['never_read']}, read in part "
            f"{pending['partly_read']}, bytes to read {pending['bytes']}"
        )
    lines.append(
        f"ingests   {count} since {since or '-'}: wall {wall:.2f} s  cpu {cpu:.2f} s"
    )
    unpriced = {
        str(r[0]): int(r[1])
        for r in db.execute("""SELECT coalesce(model, '-'), count(*) FROM requests
            WHERE dollars IS NULL AND (context_tokens > 0 OR output_tokens > 0)
            GROUP BY model ORDER BY count(*) DESC""")
    }
    lines.append(
        "no price  requests by models with no price: "
        + (", ".join(f"{k} {v}" for k, v in unpriced.items()) or "none")
    )
    other = unmatched(db)
    share = other["other_share"]
    lines.append(
        "other     commands of no kind: "
        + (f"{float(str(share)):.1%}" if share is not None else "-")
        + f" of {other['days']} days' tool time"
    )
    found_commands = other["commands"]
    assert isinstance(found_commands, list)
    if found_commands:
        lines += [
            "",
            "longest commands of no kind",
            *table(
                ["seconds", "calls", "command"],
                [
                    [c["seconds"], c["calls"], c["command"].replace("\n", " ")]
                    for c in found_commands
                ],
            ),
        ]
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
            "hosts": per_host,
            "unread": pending,
            "unmatched": other,
            "unpriced": unpriced,
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


def answer(
    args: argparse.Namespace,
    query: Callable[[sqlite3.Connection, facts.Facts, Scope], Result],
) -> Result:
    """Ingests if the store is stale, then answers `query` over the store's facts in the
    scope the arguments give."""
    fresh(args)
    db = reader()
    try:
        return query(db, facts.load(db), from_arguments(args))
    finally:
        db.close()


def run_summary(args: argparse.Namespace) -> Result:
    return answer(args, lambda _, f, s: rollups.summary(f, s))


def run_epic(args: argparse.Namespace) -> Result:
    return answer(args, lambda _, f, s: rollups.epic(f, s, str(args.id)))


def run_bead(args: argparse.Namespace) -> Result:
    return answer(args, lambda db, f, s: bead.report_from(db, f, s, str(args.id)))


def run_session(args: argparse.Namespace) -> Result:
    return answer(
        args, lambda db, f, s: session.report(db, f, s, str(args.id), args.bead)
    )


def run_time(args: argparse.Namespace) -> Result:
    return answer(args, lambda _, f, s: rollups.time_by(f, s, str(args.by)))


def run_dollars(args: argparse.Namespace) -> Result:
    return answer(args, lambda _, f, s: rollups.dollars_by(f, s, str(args.by)))


def run_cards(args: argparse.Namespace) -> Result:
    return answer(args, lambda _, f, s: cards.report(f, s, str(args.by)))


def run_machine(args: argparse.Namespace) -> Result:
    return answer(args, machine.report)


def run_ci(args: argparse.Namespace) -> Result:
    return answer(
        args,
        lambda db, f, s: ci.report(db, f, s, str(args.script), args.lane, args.family),
    )


def run_pushes(args: argparse.Namespace) -> Result:
    return answer(args, pushes.report)


def run_failures(args: argparse.Namespace) -> Result:
    return answer(args, failures.report)


def run_findings(args: argparse.Namespace) -> Result:
    return answer(args, lambda db, f, s: findings.report(db, f, s, args.rule))


def run_metrics(args: argparse.Namespace) -> Result:
    if args.name is None:
        return metrics.listing()
    name: str = str(args.name)

    def query(db: sqlite3.Connection, f: facts.Facts, s: Scope) -> Result:
        added = changes.introduced(changes.file_versions(main_checkout(Path.cwd())))
        marks = changes.marks(db, f, changes.load(), added, name)
        return metrics.series(db, f, s, name, marks)

    return answer(args, query)


def run_changes(args: argparse.Namespace) -> Result:
    """Each change's own metric spec chooses its cohort, so the query takes only `--since`,
    which chooses changes by when they reached staging."""
    fresh(args)
    db = reader()
    try:
        added = changes.introduced(changes.file_versions(main_checkout(Path.cwd())))
        return changes.report(
            db, facts.load(db), parse_since(str(args.since)), changes.load(), added
        )
    finally:
        db.close()


QUERIES: dict[str, tuple[str, Callable[[argparse.Namespace], Result]]] = {
    "summary": (
        "beads closed, per-bead medians, where time and dollars went, epics",
        run_summary,
    ),
    "epic": ("one epic's beads with their minutes and dollars", run_epic),
    "bead": ("one bead's minutes, dollars, requests and sessions", run_bead),
    "session": ("one session's requests and tool calls in order", run_session),
    "time": ("agent minutes by kind, model, tier, role, lane, epic or day", run_time),
    "dollars": (
        "dollars by class, source, purpose, model, tier, role, lane, epic or day",
        run_dollars,
    ),
    "cards": (
        "cost per card file added, with rework beside it, by tier or any label",
        run_cards,
    ),
    "machine": (
        "cores in use, saturation, time queued for cores, shared CPU, capacity",
        run_machine,
    ),
    "ci": (
        "CI's cost, catches and flakiness by script, check lane, family and check",
        run_ci,
    ),
    "pushes": (
        "the fast check per push: its seconds, CPU, repeats and steps left to slow CI",
        run_pushes,
    ),
    "failures": (
        "what each slow-CI failure cost: its Steward bead, bisection, time behind",
        run_failures,
    ),
    "findings": (
        "waste ranked by what removing it would save, per unit",
        run_findings,
    ),
    "metrics": ("the named series, or one series by day", run_metrics),
    "changes": (
        "changes made to save time or tokens, with medians before and after",
        run_changes,
    ),
    "export": (
        "write this host's rows to a compressed file for another machine",
        run_export,
    ),
    "import": ("add the rows of another machine's export to the store", run_import),
    "ingest": ("update the store from the sources", run_ingest),
    "sql": ("run a read-only SQL query against the store", run_sql),
    "coverage": (
        "what the store holds and misses, and ingest's own cost",
        run_coverage,
    ),
}
# The queries that take `--since` and cohort filters, with their default range: roll-ups
# look at the last week, drill-downs at everything.
SCOPED: dict[str, str] = {
    "summary": "7d",
    "time": "7d",
    "dollars": "7d",
    "metrics": "7d",
    "cards": "7d",
    "machine": "7d",
    "ci": "7d",
    "pushes": "7d",
    "failures": "7d",
    "findings": "7d",
    "epic": "all",
    "bead": "all",
    "session": "all",
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
        if name == "export":
            p.add_argument(
                "--since", default="all", help="24h, 7d, all or a date (default all)"
            )
            p.add_argument("--out", help="the file to write (default: in the cache)")
        if name == "import":
            p.add_argument("file", metavar="<file>", help="an export from another host")
        if name == "sql":
            p.add_argument("query", metavar="'<query>'", help="a read-only SQL query")
        if name in ("epic", "bead", "session"):
            p.add_argument("id", metavar="<id>", help=f"the {name}'s id")
        if name == "session":
            p.add_argument("--bead", help="only the rows of this bead")
        if name == "time":
            p.add_argument("--by", choices=rollups.TIME_BY, default="kind")
        if name == "dollars":
            p.add_argument("--by", choices=rollups.DOLLARS_BY, default="source")
        if name == "cards":
            p.add_argument(
                "--by",
                default="tier",
                help="tier (the default), role, lane, epic or any label's key",
            )
        if name == "ci":
            p.add_argument(
                "--script",
                default="ci",
                help="ci (slow CI, the default), check, sweep, nightly or bisect",
            )
            p.add_argument("--lane", help="only this check lane, by family")
            p.add_argument("--family", help="only this family, by check")
        if name == "changes":
            p.add_argument(
                "--since",
                default="all",
                help="changes that reached staging since: 24h, 7d, all or a date",
            )
        if name == "findings":
            p.add_argument(
                "--rule",
                choices=[r.name for r in findings.RULES],
                help="one rule's full detail",
            )
        if name == "metrics":
            p.add_argument("name", nargs="?", metavar="<name>", help="a metric's name")
        if name in SCOPED:
            add_arguments(p, SCOPED[name])
    return top


# The commands that are not queries: the page's server and its screenshots.
PAGE_COMMANDS: dict[str, str] = {
    "serve": "serve the page on 127.0.0.1 (--port N, --no-ingest)",
    "shot": "render one address of the page to a PNG (--dark, --width N)",
}


def main(argv: Sequence[str] | None = None) -> int:
    given = list(sys.argv[1:] if argv is None else argv)
    if given and given[0] in PAGE_COMMANDS:
        from . import serve  # It imports this module.

        return (serve.serve if given[0] == "serve" else serve.shot)(given[1:])
    top = parser()
    args = top.parse_args(given)
    if args.command is None:
        print("just efficiency <query>")
        for name, (summary, _) in [
            *QUERIES.items(),
            *((n, (d, None)) for n, d in PAGE_COMMANDS.items()),
        ]:
            print(f"  {name:<10} {summary}")
        return 0
    try:
        result = QUERIES[str(args.command)][1](args)
    except (sqlite3.Error, KeyError) as e:
        print(f"efficiency: {e.args[0] if e.args else e}", file=sys.stderr)
        return 1
    print(json.dumps(result.json(), indent=1) if args.json else text(result))
    return 0
