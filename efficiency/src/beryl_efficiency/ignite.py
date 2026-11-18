"""Ignite's hub as a source (docs/ignite.md, "Records and metrics"). On the machine that runs
the hub, ingest reads the hub's SQLite database read-only, by its schema
(ignite/crates/ignite_store/src/hub/schema.sql and the migrations after it):

- `records`, one row per command Ignite ran on any machine, with `compile_units`, the
  compiler's cost per crate. They are read after the last record ID already read and stand in
  for the run log, the CPU token pool's leases and the CPU sampler: each record becomes a row
  of `ignite_records`, one `leases` row (its wait for cores), `cpu` and `cpu_minutes` rows
  spread over the minutes it ran, and, for a check family, a `checks` and a `lane_runs` row.
  The gate (class `land`) is the script `check`, as the fast check was, and tip verification
  (class `tip`) is `ci`, as slow CI was; other classes keep their name. A record's `at` is
  when the command ended. Each compile unit becomes a `compile_timings` row. Enact's lock log
  has no counterpart: nothing in Ignite waits on one lock.
- `metrics`, the efficiency rows other machines' daemons posted, one export line per row.
  They are imported as `just efficiency import` imports a file.
- `tickets`, `tips`, `handoffs`, `regressions` and `workers`, copied whole into the
  `ignite_*` tables when they change, for the series in `ignite_metrics.py`.

A machine is named by the first host name seen for its ID (`ignite_machines`), from the hub's
`machines` table or an export's header, so renaming a machine does not split its history.
"""

from __future__ import annotations

import json
import os
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import exchange, kinds, ticket_ids, timing
from .cilog import CHECK_SUMS, LANE_RUN_SUMS, host_name, iso, local_day, utc
from .cpu import join as join_leases
from .ingest import CursorSource, Merge, Row, SnapshotSource, SourceError, write

# How many records or imports one page reads.
PAGE = 5000
# The record's class as the `script` of the `checks` and `lane_runs` rows.
SCRIPTS = {"land": "check", "tip": "ci"}
# A record with this verb is a check family the result cache answered without running it.
CACHED = "cached"
RAN = {"pass", "fail", "flaky"}
MERGES: dict[str, Merge] = {
    "checks": CHECK_SUMS,
    "lane_runs": LANE_RUN_SUMS,
    "cpu": {"seconds": "max", "cpu_seconds": "sum"},
    "cpu_minutes": {
        "seconds": "max",
        "cores": "max",
        "cpu_seconds": "sum",
        "peak_cores": "max",
    },
}


def hub_path(cache: Path) -> Path:
    """The hub's database: `$IGNITE_HUB_DB`, else `<cache>/ignite/hub.sqlite`."""
    found = os.environ.get("IGNITE_HUB_DB")
    return Path(found) if found else cache / "ignite" / "hub.sqlite"


def open_hub(path: Path) -> sqlite3.Connection:
    if not path.exists():
        raise SourceError(f"no hub database at {path}")
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    return db


def machine_host(db: sqlite3.Connection, machine: str, seen: str) -> str:
    """The host a machine's rows are stored under: the first one seen for its ID."""
    db.execute(
        "INSERT OR IGNORE INTO ignite_machines (id, host) VALUES (?, ?)",
        (machine, host_name(seen)),
    )
    return str(
        db.execute(
            "SELECT host FROM ignite_machines WHERE id = ?", (machine,)
        ).fetchone()[0]
    )


def minutes(start: datetime, end: datetime) -> list[tuple[datetime, float]]:
    """The minutes from `start` to `end`, each with the share of the span inside it."""
    span = (end - start).total_seconds()
    if span <= 0:
        return [(start.replace(second=0, microsecond=0), 1.0)]
    found: list[tuple[datetime, float]] = []
    minute = start.replace(second=0, microsecond=0)
    while minute < end:
        following = minute + timedelta(minutes=1)
        inside = (min(end, following) - max(start, minute)).total_seconds()
        found.append((minute, inside / span))
        minute = following
    return found


def record_rows(r: dict[str, object], host: str, cores: int) -> list[Row]:
    """The store's rows for one record (see the module's docstring)."""
    end = utc(r["at"])
    wall, cpu, queued = (float(str(r[k])) for k in ("wall_s", "cpu_s", "queued_s"))
    start = end - timedelta(seconds=wall)
    tree = str(r["tree"] or "")
    verb, klass, family = str(r["verb"]), str(r["class"]), r["family"]
    outcome = str(r["outcome"] or "")
    rows: list[Row] = [
        (
            "ignite_records",
            {
                "host": host,
                "machine": r["machine"],
                "id": r["id"],
                "ticket": r["ticket"],
                "session": r["session"],
                "tree": r["tree"],
                "verb": verb,
                "class": klass,
                "family": family,
                "queued_seconds": queued,
                "wall_seconds": wall,
                "cpu_seconds": cpu,
                "instructions": r["instructions"],
                "peak_mb": r["peak_mb"],
                "outcome": r["outcome"],
                "commit": r["commit_sha"],
                "at": iso(end),
            },
        )
    ]
    if verb == CACHED:
        return rows
    rows.append(
        (
            "leases",
            {
                "host": host,
                "time": iso(end),
                "class": klass,
                "worktree": tree,
                "command": f"{verb} {family}" if family else verb,
                "cores": 1,
                "wanted": 1,
                "waited_seconds": queued,
                "held_seconds": wall,
                "wait_start": iso(start - timedelta(seconds=queued)),
                "wait_end": iso(start),
            },
        )
    )
    for minute, share in minutes(start, end):
        rows.append(
            (
                "cpu",
                {
                    "host": host,
                    "time": iso(minute),
                    "owner": str(r["session"] or "ignite"),
                    "kind": verb,
                    "worktree": tree,
                    "seconds": 60.0,
                    "cpu_seconds": cpu * share,
                },
            )
        )
        rows.append(
            (
                "cpu_minutes",
                {
                    "host": host,
                    "time": iso(minute),
                    "seconds": 60.0,
                    "cores": cores,
                    "cpu_seconds": cpu * share,
                    "peak_cores": cpu / wall if wall > 0 else 0.0,
                },
            )
        )
    if family:
        ran = outcome in RAN
        script = SCRIPTS.get(klass, klass)
        common = {
            "seconds": wall if ran else 0.0,
            "cpu_seconds": cpu if ran else 0.0,
            "measured_seconds": wall if ran else 0.0,
        }
        rows += [
            (
                "checks",
                {
                    "day": local_day(end),
                    "host": host,
                    "script": script,
                    "lane": family,
                    "check": family,
                    "first": iso(end),
                    "runs": int(ran),
                    "passes": int(outcome == "pass"),
                    "fails": int(outcome == "fail"),
                    "skips": int(not ran),
                    **common,
                },
            ),
            (
                "lane_runs",
                {
                    "host": host,
                    "script": script,
                    "worktree": tree,
                    "commit": str(r["commit_sha"] or ""),
                    "tree": verb,
                    "lane": family,
                    "start": iso(start),
                    "end": iso(end),
                    "checks": int(ran),
                    "fails": int(outcome == "fail"),
                    **common,
                },
            ),
        ]
    return rows


class Records(CursorSource):
    """The hub's records and compile units, after the last record ID read."""

    name = "ignite-records"

    def __init__(self, path: Path, local: str) -> None:
        self.path = path
        self.local = local

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        hub = open_hub(self.path)
        try:
            found = [
                dict(r)
                for r in hub.execute(
                    """SELECT r.*, m.name AS machine_name, m.cores AS machine_cores
                    FROM records AS r LEFT JOIN machines AS m ON m.id = r.machine
                    WHERE r.id > ? ORDER BY r.id LIMIT ?""",
                    (after, PAGE),
                )
            ]
            for r in found:
                r["units"] = [
                    dict(u)
                    for u in hub.execute(
                        "SELECT * FROM compile_units WHERE record = ?", (r["id"],)
                    )
                ]
        finally:
            hub.close()
        if not found:
            return b"", after, [], 0
        data = json.dumps(found).encode()
        return data, int(str(found[-1]["id"])), [("_record", r) for r in found], 0

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        """Names each record's machine, writes its rows and joins its waits for cores to the
        tool calls that ran them, as the token pool's leases were."""
        written: list[Row] = []
        hosts: set[str] = set()
        for _, r in rows:
            machine = str(r["machine"])
            host = machine_host(db, machine, str(r["machine_name"] or machine))
            hosts.add(host)
            if r["ticket"] is not None:
                r["ticket"] = ticket_ids.stored(db, str(r["ticket"]))
            written += record_rows(r, host, int(str(r["machine_cores"] or 0)))
            units = r["units"]
            assert isinstance(units, list)
            for u in units:
                written.append(
                    (
                        "compile_timings",
                        {
                            "host": host,
                            "run": f"{machine}:{r['id']}",
                            "time": iso(utc(r["at"])),
                            "crate": u["crate_name"],
                            "mode": u["mode"],
                            "seconds": u["cpu_s"],
                            "fresh": int(not u["rebuilt"]),
                        },
                    )
                )
        count = write(db, written, MERGES)
        if self.local in hosts:
            table, _ = kinds.load()
            heavy = frozenset(k.kind for k in table if k.heavy)
            for provider, transcript in sorted(join_leases(db, self.local)):
                count += timing.account(db, self.local, provider, transcript, heavy)
        return count


class Imports(CursorSource):
    """The efficiency rows other machines posted to the hub. The hub stores each posted line
    once, in `metrics`, with when it first arrived, so the cursor is the latest arrival read,
    in milliseconds. A page reads every row that arrived at or after it (the rows of one post
    share their arrival) and ends the stream when nothing arrived later."""

    name = "ignite-imports"

    def __init__(self, path: Path, local: str) -> None:
        self.path = path
        self.local = local

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        since = iso(datetime.fromtimestamp(after / 1000, timezone.utc))
        hub = open_hub(self.path)
        try:
            found = hub.execute(
                "SELECT machine, source, data, at FROM metrics WHERE at >= ? ORDER BY at",
                (since if after else "",),
            ).fetchall()
        finally:
            hub.close()
        if not found:
            return b"", after, [], 0
        cursor = max(int(utc(r["at"]).timestamp() * 1000) for r in found)
        groups: dict[tuple[str, str], list[str]] = {}
        for r in found:
            groups.setdefault((str(r["machine"]), str(r["source"])), []).append(
                str(r["data"])
            )
        data = "".join(str(r["data"]) for r in found).encode()
        rows: list[Row] = []
        for (machine, source), lines in groups.items():
            group: dict[str, object] = {
                "machine": machine,
                "source": source,
                "lines": lines,
            }
            rows.append(("_import", group))
        return data, cursor, rows, 0

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        """Imports each machine's rows under its name, as `import` imports a file, with the
        newest-schema header the machine sent first. The hub's own machine posts its rows
        too; those are skipped, since ingest here read them already."""
        count = 0
        for _, r in rows:
            host = machine_host(db, str(r["machine"]), str(r["source"]))
            if host == self.local:
                continue
            lines = r["lines"]
            assert isinstance(lines, list)
            parsed = [json.loads(str(line)) for line in lines]
            headers = [p for p in parsed if isinstance(p, dict) and "export" in p]
            if not headers:
                continue
            header = max(headers, key=lambda h: int(h.get("schema") or 0))
            body = [
                json.dumps(p) for p in parsed if isinstance(p, dict) and "table" in p
            ]
            try:
                found = exchange.import_lines(
                    db, [json.dumps(header), *body], self.local, lambda _: host
                )
            except exchange.ExchangeError:
                continue
            count += sum(found.added.values())
        return count


class Hub(SnapshotSource):
    """The hub's tickets, tips, hand-offs and workers, copied whole when they change."""

    name = "ignite-hub"
    owns = ("ignite_tickets", "ignite_tips", "ignite_handoffs", "ignite_workers")

    def __init__(self, path: Path) -> None:
        self.path = path

    def read(self) -> bytes:
        hub = open_hub(self.path)
        try:
            repairs = {
                str(r[0])
                for r in hub.execute(
                    "SELECT ticket FROM regressions UNION "
                    "SELECT ticket FROM handoffs WHERE kind = 'regression'"
                )
            }
            found = {
                "tickets": [
                    [r["id"], r["created_at"], r["claimed_at"], r["landed_at"]]
                    + [int(r["id"] in repairs)]
                    for r in hub.execute(
                        "SELECT id, created_at, claimed_at, landed_at FROM tickets"
                    )
                ],
                "tips": [list(r) for r in hub.execute("SELECT * FROM tips")],
                "handoffs": [list(r) for r in hub.execute("SELECT * FROM handoffs")],
                "workers": [
                    list(r)
                    for r in hub.execute("SELECT machine, session, role FROM workers")
                ],
            }
        except sqlite3.Error as e:
            raise SourceError(f"reading the hub: {e}") from e
        finally:
            hub.close()
        return json.dumps(found, sort_keys=True).encode()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        found = json.loads(data)
        columns = {
            "tickets": ("id", "created_at", "claimed_at", "landed_at", "repair"),
            "tips": ("commit", "seen", "verified", "state"),
            "handoffs": ("seq", "machine", "ticket", "kind", "at"),
            "workers": ("machine", "session", "role"),
        }
        rows: list[Row] = [
            (f"ignite_{table}", dict(zip(names, values)))
            for table, names in columns.items()
            for values in found[table]
        ]
        return rows, 0


def sources(cache: Path, local: str) -> list[Hub | Records | Imports]:
    """The hub's sources, or none on a machine that holds no hub database."""
    path = hub_path(cache)
    if not path.exists():
        return []
    return [Hub(path), Records(path, local), Imports(path, local)]
