"""Ignite's hub as a source (docs/ignite.md, "Records and metrics"). On the machine that runs
the hub, ingest reads the hub's SQLite database read-only, by its schema
(ignite/crates/ignite_store/src/hub/schema.sql and the migrations after it):

- `records`, one row per command Ignite ran on any machine. Ingest resumes at the last record
  ID it read. These records stand in for the run log, the CPU token pool's leases and the
  CPU sampler. Each becomes an `ignite_records` row, one `leases` row (its wait for cores),
  `cpu` and `cpu_minutes` rows spread over the minutes it ran, and, for a check family, a
  `checks` and a `lane_runs` row.
  The gate (class `land`) is the script `check`, as the fast check was, and tip verification
  (class `tip`) is `ci`, as slow CI was. Other classes keep their name. A record's `at`
  is when the command ended.
- `metrics`, the efficiency rows other machines' daemons posted, one export line per row.
  They are imported as `just efficiency import` imports a file.
- `metrics` rows of the source `plan_usage`, the Claude plan's usage samples a dispatcher
  took, one row per window of the plan (`PlanSamples`), which `plan.py` fits to dollars.
- `tickets`, `tips`, `handoffs`, `regressions` and `workers`, copied whole into the
  `ignite_*` tables when they change, for the series in `ignite_metrics.py`. The tickets,
  with their labels and aliases, are also written as `beads`, `labels` and `ticket_aliases`
  rows, which every per-ticket query reads, so that the hub replaces `tk export`.

A machine is named by the first host name seen for its ID (`ignite_machines`), from the hub's
`machines` table or an export's header, so renaming a machine does not split its history.
Away from the hub, the daemon's ticket mirror supplies tickets and regressions. Its local
database supplies check pass timings, tip rounds and plan usage samples.
"""

from __future__ import annotations

import json
import os
import sqlite3
from collections.abc import Sequence
from contextlib import closing
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import beads, exchange, kinds, store, ticket_ids, timing
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
LOCAL_CHECKS = "local"
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


def ignite_home() -> Path:
    home = os.environ.get("IGNITE_HOME")
    return Path(home) if home else Path.home() / ".config" / "ignite"


def hub_path(cache: Path) -> Path:
    """The hub's database: `$IGNITE_HUB_DB`, else where the hub's daemon keeps it,
    `hub/hub.db` in the Ignite home (`$IGNITE_HOME`, else `~/.config/ignite`). `cache` is
    unused; the hub never lived in Beryl's cache, and an old default there made every
    Ignite table come out empty."""
    del cache
    found = os.environ.get("IGNITE_HUB_DB")
    if found:
        return Path(found)
    return ignite_home() / "hub" / "hub.db"


def open_ignite_db(path: Path) -> sqlite3.Connection:
    if not path.exists():
        raise SourceError(f"no Ignite database at {path}")
    db = store.open_read_only(path)
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
    """The hub's records after the last record ID read."""

    name = "ignite-records"

    def __init__(self, path: Path, local: str) -> None:
        self.path = path
        self.local = local

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        hub = open_ignite_db(self.path)
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
        hub = open_ignite_db(self.path)
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
        too; those are skipped, since ingest here read them already. Rows of another
        source, such as `findings` or `plan_usage`, carry no export header and are skipped
        before the machine is named, since their source is not a host name."""
        count = 0
        for _, r in rows:
            lines = r["lines"]
            assert isinstance(lines, list)
            parsed = [json.loads(str(line)) for line in lines]
            headers = [p for p in parsed if isinstance(p, dict) and "export" in p]
            if not headers:
                continue
            host = machine_host(db, str(r["machine"]), str(r["source"]))
            if host == self.local:
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


def resets_text(value: object) -> str | None:
    """A window's `resetsAt` as a UTC ISO time. The app may report it as text or as seconds
    (or milliseconds) since the epoch; anything else is kept as it came."""
    if value is None:
        return None
    if isinstance(value, (int, float)) or (
        isinstance(value, str) and value.replace(".", "", 1).isdigit()
    ):
        number = float(value)
        if number > 1e11:
            number /= 1000
        return iso(datetime.fromtimestamp(number, timezone.utc))
    text = str(value)
    try:
        return iso(utc(text))
    except (ValueError, TypeError):
        return text


class PlanSamples(CursorSource):
    """Per-provider plan usage samples in the hub's `metrics` (source `plan_usage`), after
    the latest arrival read, in milliseconds, as `Imports` reads. A sample whose status is
    not `ok` holds no window and is left out."""

    name = "ignite-plan-usage"
    SOURCE = "plan_usage"

    def __init__(self, path: Path) -> None:
        self.path = path

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        since = iso(datetime.fromtimestamp(after / 1000, timezone.utc))
        hub = open_ignite_db(self.path)
        try:
            found = hub.execute(
                """SELECT x.machine, coalesce(m.name, x.machine) AS name, x.data, x.at
                FROM metrics AS x LEFT JOIN machines AS m ON m.id = x.machine
                WHERE x.source = ? AND x.at > ? ORDER BY x.at""",
                (self.SOURCE, since if after else ""),
            ).fetchall()
        finally:
            hub.close()
        if not found:
            return b"", after, [], 0
        cursor = max(int(utc(r["at"]).timestamp() * 1000) for r in found)
        rows: list[Row] = []
        skipped = 0
        for r in found:
            try:
                data = json.loads(str(r["data"]))
                percent, window = data.get("percent_used"), data.get("window")
                if data.get("status") != "ok" or percent is None or not window:
                    continue
                provider = data.get("provider")
                rows.append(
                    (
                        "_sample",
                        {
                            "machine": str(r["machine"]),
                            "name": str(r["name"]),
                            "provider": (
                                str(provider)
                                if isinstance(provider, str) and provider
                                else "claude"
                            ),
                            "at": iso(utc(data["at"])),
                            "window": str(window),
                            "percent_used": float(percent),
                            "resets_at": resets_text(data.get("resets_at")),
                        },
                    )
                )
            except (ValueError, TypeError, KeyError, AttributeError):
                skipped += 1
        data_bytes = "".join(str(r["data"]) for r in found).encode()
        return data_bytes, cursor, rows, skipped

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        written: list[Row] = []
        for _, r in rows:
            host = machine_host(db, str(r["machine"]), str(r.pop("name")))
            written.append(("plan_usage", {"host": host, **r}))
        return write(db, written)


class Hub(SnapshotSource):
    """The hub's tickets, tips, hand-offs and workers, copied whole when they change."""

    name = "ignite-hub"
    owns = ("ignite_tickets", "ignite_tips", "ignite_handoffs", "ignite_workers")

    def __init__(self, path: Path) -> None:
        self.path = path

    def read(self) -> bytes:
        hub = open_ignite_db(self.path)
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


def legacy_id(ticket: str, aliases: list[str]) -> str:
    """The ID the store keeps a ticket under: the `by-` alias an imported ticket had in the
    earlier tracker, else the ticket's own ID with that prefix (`ticket_ids.prefixed`).
    """
    own = ticket_ids.prefixed(ticket)
    legacy = [a for a in aliases if a.startswith(ticket_ids.PREFIX)]
    return own if own in legacy else next(iter(legacy), own)


class HubTickets(SnapshotSource):
    """The hub's tickets as the `beads`, `labels` and `ticket_aliases` rows, as `tk export`
    once supplied them. It owns no table: stored tickets that the hub no longer lists stay,
    and so does a label removed from a ticket, since the stored history is kept."""

    name = "ignite-beads"

    def __init__(self, path: Path) -> None:
        self.path = path

    def read(self) -> bytes:
        hub = open_ignite_db(self.path)
        try:
            aliases: dict[str, list[str]] = {}
            for alias, ticket in hub.execute("SELECT alias, id FROM aliases"):
                aliases.setdefault(str(ticket), []).append(str(alias))
            found = [
                {**dict(r), "aliases": sorted(aliases.get(str(r["id"]), []))}
                for r in hub.execute(
                    "SELECT id, parent, title, type, status, priority, labels, metadata, "
                    "created_at, claimed_at, closed_at, tree, session, landed FROM tickets"
                )
            ]
        except sqlite3.Error as e:
            raise SourceError(f"reading the hub: {e}") from e
        finally:
            hub.close()
        return json.dumps(found, sort_keys=True).encode()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        records = json.loads(data)
        names = {str(r["id"]): legacy_id(str(r["id"]), r["aliases"]) for r in records}
        rows: list[Row] = []
        skipped = 0
        for r in records:
            ticket = str(r["id"])
            bead = names[ticket]
            for alias in (
                ticket,
                f"T{ticket}",
                ticket_ids.prefixed(ticket),
                *r["aliases"],
            ):
                rows.append(("ticket_aliases", {"alias": alias, "bead": bead}))
            try:
                metadata = json.loads(r["metadata"] or "{}")
                landed = json.loads(r["landed"] or "{}")
                metadata.update(
                    {
                        "enact_commits": " ".join(landed.get("commits") or []),
                        "enact_worktree": r["tree"],
                        "enact_session": r["session"],
                    }
                )
                parent = r["parent"]
                rows += beads.rows_of(
                    {
                        "id": bead,
                        "title": r["title"],
                        "parent": names.get(str(parent), parent),
                        "issue_type": r["type"],
                        "status": r["status"],
                        "priority": r["priority"],
                        "created_at": r["created_at"],
                        "started_at": r["claimed_at"],
                        "closed_at": r["closed_at"],
                        "labels": json.loads(r["labels"] or "[]"),
                        "metadata": {k: v for k, v in metadata.items() if v},
                    }
                )
            except (ValueError, TypeError, KeyError, AttributeError):
                skipped += 1
        return rows, skipped


class MirrorTickets(HubTickets):
    """The newest version of each ticket in the daemon's append-only ticket mirror."""

    name = "ignite-mirror"

    def read(self) -> bytes:
        try:
            with closing(open_ignite_db(self.path)) as mirror:
                found = []
                for (data,) in mirror.execute(
                    "SELECT whole_json FROM versions WHERE id IN "
                    "(SELECT max(id) FROM versions GROUP BY ticket) ORDER BY ticket"
                ):
                    whole = json.loads(data)
                    ticket = {
                        field: whole.get(field)
                        for field in (
                            "id",
                            "parent",
                            "title",
                            "type",
                            "status",
                            "priority",
                            "created_at",
                            "claimed_at",
                            "closed_at",
                            "landed_at",
                            "tree",
                            "session",
                        )
                    }
                    ticket["aliases"] = sorted(whole.get("aliases", []))
                    metadata = whole.get("metadata") or {}
                    ticket["metadata"] = json.dumps(
                        {
                            **metadata,
                            "enact_regression_check": metadata.get("culprit_check"),
                        }
                    )
                    for field in ("labels", "landed"):
                        value = whole.get(field)
                        ticket[field] = json.dumps(value) if value is not None else None
                    found.append(ticket)
        except (sqlite3.Error, ValueError, TypeError) as e:
            raise SourceError(f"reading Ignite's ticket mirror: {e}") from e
        return json.dumps(found, sort_keys=True).encode()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        rows, skipped = super().parse(data)
        for ticket in json.loads(data):
            rows.append(
                (
                    "ignite_tickets",
                    {
                        "id": ticket["id"],
                        "created_at": ticket["created_at"],
                        "claimed_at": ticket["claimed_at"],
                        "landed_at": ticket["landed_at"],
                        "repair": int(
                            "regression" in json.loads(ticket["labels"] or "[]")
                        ),
                    },
                )
            )
        return rows, skipped


class DaemonPasses(CursorSource):
    """Recorded wall times for successful commands in local/schema_5.sql."""

    name = "ignite-local-passes"

    def __init__(self, path: Path, host: str) -> None:
        self.path = path
        self.host = host

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        try:
            with closing(open_ignite_db(self.path)) as daemon:
                found = daemon.execute(
                    "SELECT rowid, family, wall_s, at FROM passes "
                    "WHERE rowid > ? AND command = 'run' ORDER BY rowid LIMIT ?",
                    (after, PAGE),
                ).fetchall()
        except sqlite3.Error as e:
            raise SourceError(f"reading Ignite's check pass timings: {e}") from e
        if not found:
            return b"", after, [], 0
        rows: list[Row] = [
            (
                "checks",
                {
                    "day": local_day(utc(at)),
                    "host": self.host,
                    "script": LOCAL_CHECKS,
                    "lane": family,
                    "check": family,
                    "first": iso(utc(at)),
                    "runs": 1,
                    "passes": 1,
                    "fails": 0,
                    "skips": 0,
                    "seconds": wall,
                    "cpu_seconds": 0.0,
                    "measured_seconds": 0.0,
                },
            )
            for _, family, wall, at in found
        ]
        return json.dumps([list(r) for r in found]).encode(), int(found[-1][0]), rows, 0

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        return write(db, rows, {"checks": CHECK_SUMS})


class DaemonMetrics(SnapshotSource):
    """The finished tip rounds and plan samples retained in the local daemon database."""

    name = "ignite-local-metrics"

    def __init__(self, path: Path, host: str) -> None:
        self.path = path
        self.host = host

    def read(self) -> bytes:
        try:
            with closing(open_ignite_db(self.path)) as daemon:
                tips = [
                    list(r)
                    for r in daemon.execute(
                        """SELECT json_extract(reply, '$.json.tip'), started_at, finished_at,
                            json_extract(reply, '$.json.outcome.state')
                        FROM handles WHERE verb = 'tip-round' AND state = 'done'
                        AND exit = 0 AND finished_at IS NOT NULL
                        AND json_extract(reply, '$.json.outcome.state')
                            IN ('verified', 'verified_with_deferrals', 'failed')
                        ORDER BY finished_at, id"""
                    )
                ]
                samples = [
                    list(r)
                    for r in daemon.execute(
                        """SELECT machine, provider, at, label, percent_used, resets_at
                        FROM plan_usage WHERE status = 'ok' AND label IS NOT NULL
                        AND percent_used IS NOT NULL ORDER BY id"""
                    )
                ]
        except sqlite3.Error as e:
            raise SourceError(f"reading Ignite's local metrics: {e}") from e
        return json.dumps({"tips": tips, "samples": samples}).encode()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        found = json.loads(data)
        rows: list[Row] = [
            (
                "ignite_tip_rounds",
                {
                    "host": self.host,
                    "commit": commit,
                    "started_at": iso(utc(start)),
                    "finished_at": iso(utc(end)),
                    "state": state,
                },
            )
            for commit, start, end, state in found["tips"]
        ]
        rows.extend(
            (
                "plan_usage",
                {
                    "host": self.host,
                    "machine": machine,
                    "provider": provider,
                    "at": iso(utc(at)),
                    "window": window,
                    "percent_used": percent,
                    "resets_at": resets_text(resets),
                },
            )
            for machine, provider, at, window, percent, resets in found["samples"]
        )
        return rows, 0


def ticket_sources(cache: Path) -> Sequence[HubTickets]:
    """Ingest reads tickets before transcripts, whose windows refer to them."""
    path = hub_path(cache)
    if path.is_file():
        return [HubTickets(path)]
    mirror = ignite_home() / "mirror.db"
    return [MirrorTickets(mirror)] if mirror.is_file() else []


def reads_fleet(cache: Path) -> bool:
    """Whether ingest here reads every machine's requests, as it does at the hub through
    `Imports`. A plan sample's percent counts the whole account, so `plan.py` can only fit
    it to dollars in a store that holds them all."""
    return hub_path(cache).is_file()


def sources(cache: Path, local: str) -> Sequence[CursorSource | SnapshotSource]:
    """Read fleet records at the hub and the local daemon's measurements elsewhere."""
    path = hub_path(cache)
    if path.is_file():
        return [
            Hub(path),
            Records(path, local),
            Imports(path, local),
            PlanSamples(path),
        ]
    daemon = ignite_home() / "daemon.db"
    return (
        [DaemonPasses(daemon, local), DaemonMetrics(daemon, local)]
        if daemon.is_file()
        else []
    )
