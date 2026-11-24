"""Ignite's fleet data on a machine without the hub's database (docs/efficiency-dashboard.md,
"Other machines"). The hub's private listener serves three reads to a registered machine
(ignite/crates/ignite_hub/src/routes/efficiency.rs), each answered with the rows that
`ignite.py` reads from the hub's database on the hub's own machine:

- `GET /efficiency/records?after=<id>` pages through the command records in ID order, each
  with its machine's name and cores, and `next_id`. `Records` writes them as
  `ignite.Records` does.
- `GET /efficiency/metrics?cursor=<token>` pages through the efficiency rows that machines
  posted, with the export header of each machine and source on the page, and `next_cursor`.
  `Metrics` imports each machine's rows as `ignite.Imports` does and reads the `plan_usage`
  samples as `ignite.PlanSamples` does. The cursor is kept exactly as the hub sent it.
- `GET /efficiency/state` returns the hub's tickets, tips, hand-offs, workers and
  regressions, which `State` copies as `ignite.Hub` does.

The hub's address is `[hub] url` and `project` in the `ignite.toml` above the checkout, and
the machine's token is `$IGNITE_TOKEN`, else the `token` in `machine.json` in the Ignite
home, else the file `token` beside it, as the `ignite` command finds them
(ignite/crates/ignite_cli/src/ticket/http.rs, `Hub::load`).

The local daemon's database (`daemon.db` in the Ignite home) supplies this machine's check
pass times, finished tip rounds and plan usage samples. Its pass times are the script
`local`, so they never add to the fleet's `ci` and `check` totals. When the hub cannot be
reached, its sources record an error and write nothing, their positions stay where the last
page left them, and the local sources still answer.
"""

from __future__ import annotations

import http.client
import json
import os
import sqlite3
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable, Mapping, Sequence
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path

from .cilog import CHECK_SUMS, iso, local_day, utc
from .ignite import (
    LOCAL_CHECKS,
    MALFORMED_SAMPLE,
    PAGE,
    STATE_TABLES,
    hub_path,
    ignite_home,
    open_ignite_db,
    resets_text,
    sample_rows,
    save_imports,
    save_records,
    save_samples,
    state_rows,
)
from .ingest import (
    CursorSource,
    Row,
    SnapshotSource,
    Source,
    SourceError,
    TokenSource,
    write,
)

# The name the hub records for these reads (`X-Actor`).
ACTOR = "efficiency"
# How long one request may take, in seconds, before the hub counts as unreachable. A query
# ingests first, so an unreachable hub delays it by this once (see `Hub.unreachable`).
TIMEOUT = 10.0
# Addresses on this machine, for which Ignite ignores any configured proxy.
LOOPBACK = ("127.0.0.1", "localhost", "[::1]", "::1")


class NoRedirects(urllib.request.HTTPRedirectHandler):
    """Refuses redirects, as Ignite's client does, so the token never goes elsewhere."""

    def redirect_request(
        self,
        req: urllib.request.Request,
        fp: object,
        code: int,
        msg: str,
        headers: object,
        newurl: str,
    ) -> None:
        return None


@dataclass
class Hub:
    """The hub's API at `base` (`<url>/<project>/api`), read with the machine's token. Once a
    request cannot reach the hub, `unreachable` holds why and later requests fail at once, so
    one ingest waits for the hub's timeout only once."""

    base: str
    token: str | None
    unreachable: str = ""

    def get(
        self, route: str, query: Mapping[str, str]
    ) -> tuple[bytes, dict[str, object]]:
        """The body and JSON object of `GET /efficiency/<route>?<query>`. Raises `SourceError`
        when there is no token, the hub cannot be reached or it answers with an error.
        """
        if self.unreachable:
            raise SourceError(self.unreachable)
        if not self.token:
            raise SourceError(
                "no registered machine token for the hub; run `ignite install`"
            )
        url = f"{self.base}/efficiency/{route}"
        if query:
            url += "?" + urllib.parse.urlencode(query)
        request = urllib.request.Request(
            url,
            headers={"Authorization": f"Bearer {self.token}", "X-Actor": ACTOR},
        )
        handlers: list[urllib.request.BaseHandler] = [NoRedirects()]
        if urllib.parse.urlsplit(url).hostname in LOOPBACK:
            handlers.append(urllib.request.ProxyHandler({}))
        try:
            with urllib.request.build_opener(*handlers).open(
                request, timeout=TIMEOUT
            ) as response:
                data = response.read()
        except urllib.error.HTTPError as e:
            raise SourceError(f"the hub answered {route} with HTTP {e.code}") from e
        except (urllib.error.URLError, http.client.HTTPException, OSError) as e:
            self.unreachable = f"cannot reach the hub at {self.base}: {e}"
            raise SourceError(self.unreachable) from e
        try:
            found = json.loads(data)
        except ValueError as e:
            raise SourceError(f"the hub's {route} answer is not JSON: {e}") from e
        if not isinstance(found, dict):
            raise SourceError(f"the hub's {route} answer is not an object")
        return data, found


def config_file(start: Path) -> Path | None:
    """The `ignite.toml` in `start` or the nearest directory above it."""
    for directory in (start, *start.parents):
        found = directory / "ignite.toml"
        if found.is_file():
            return found
    return None


def machine_token(env: Callable[[str], str | None]) -> str | None:
    """The machine's token, found where the `ignite` command finds it."""
    found = env("IGNITE_TOKEN") or None
    home = ignite_home()
    if found is None:
        try:
            identity = json.loads((home / "machine.json").read_text(encoding="utf-8"))
            token = identity.get("token") if isinstance(identity, dict) else None
            found = token if isinstance(token, str) else None
        except (OSError, ValueError):
            found = None
    if found is None:
        try:
            found = (home / "token").read_text(encoding="utf-8")
        except OSError:
            found = None
    return found.strip() or None if found is not None else None


def load(start: Path, env: Callable[[str], str | None] = os.environ.get) -> Hub | None:
    """The hub named by the `ignite.toml` above `start`, or None where there is none."""
    path = config_file(start)
    if path is None:
        return None
    try:
        config = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    hub = config.get("hub")
    if not isinstance(hub, dict):
        return None
    url, project = hub.get("url"), hub.get("project")
    if not isinstance(url, str) or not isinstance(project, str):
        return None
    return Hub(f"{url.rstrip('/')}/{project}/api", machine_token(env))


class Records(CursorSource):
    """The hub's command records after the last record ID read."""

    name = "ignite-fleet-records"

    def __init__(self, hub: Hub, local: str) -> None:
        self.hub = hub
        self.local = local

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        query = {"limit": str(PAGE)}
        if after:
            query["after"] = str(after)
        data, found = self.hub.get("records", query)
        records = found.get("records")
        following = found.get("next_id")
        if not isinstance(records, list) or not isinstance(following, int):
            raise SourceError("the hub's records answer has no records or next_id")
        rows: list[Row] = [("_record", r) for r in records if isinstance(r, dict)]
        return data, following, rows, len(records) - len(rows)

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        return save_records(db, rows, self.local)


class Metrics(TokenSource):
    """The efficiency rows machines posted to the hub, after the hub's last cursor read.
    Rows with an export header are another machine's store rows; `plan_usage` rows are plan
    samples; rows of other sources, such as `findings`, are for the hub and are skipped.
    """

    name = "ignite-fleet-metrics"

    def __init__(self, hub: Hub, local: str) -> None:
        self.hub = hub
        self.local = local

    def page(self, token: str) -> tuple[bytes, str, list[Row], int]:
        query = {"limit": str(PAGE)}
        if token:
            query["cursor"] = token
        data, found = self.hub.get("metrics", query)
        entries, headers = found.get("rows"), found.get("headers")
        following = found.get("next_cursor")
        if (
            not isinstance(entries, list)
            or not isinstance(headers, list)
            or not isinstance(following, str)
        ):
            raise SourceError("the hub's metrics answer has no rows or next_cursor")
        groups: dict[tuple[str, str], list[str]] = {}
        for h in headers:
            if isinstance(h, dict):
                groups[(str(h["machine"]), str(h["source"]))] = [str(h["data"])]
        rows: list[Row] = []
        skipped = 0
        for entry in entries:
            try:
                machine, source = str(entry["machine"]), str(entry["source"])
                if source == "plan_usage":
                    name = entry.get("machine_name") or machine
                    rows += sample_rows(machine, str(name), str(entry["data"]))
                else:
                    groups.setdefault((machine, source), []).append(str(entry["data"]))
            except MALFORMED_SAMPLE:
                skipped += 1
        for (machine, source), lines in groups.items():
            group: dict[str, object] = {
                "machine": machine,
                "source": source,
                "lines": lines,
            }
            rows.append(("_import", group))
        return data, following, rows, skipped

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        samples = [r for r in rows if r[0] == "_sample"]
        imports = [r for r in rows if r[0] == "_import"]
        return save_samples(db, samples) + save_imports(db, imports, self.local)


class State(SnapshotSource):
    """The hub's tickets, tips, hand-offs, workers and regressions, copied whole when they
    change."""

    name = "ignite-fleet-state"
    owns: tuple[str, ...] = STATE_TABLES

    def __init__(self, hub: Hub) -> None:
        self.hub = hub

    def read(self) -> bytes:
        _, found = self.hub.get("state", {})
        return json.dumps(found, sort_keys=True).encode()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        try:
            return state_rows(json.loads(data)), 0
        except (KeyError, TypeError, ValueError, AssertionError) as e:
            raise SourceError(f"the hub's state answer is malformed: {e!r}") from e


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


def sources(checkout: Path, cache: Path, local: str) -> Sequence[Source]:
    """On a machine without the hub's database, the hub's fleet data over the network where an
    `ignite.toml` above `checkout` names the hub, and the local daemon's measurements. On the
    hub's own machine nothing, since `ignite.sources` reads the hub's database there."""
    if hub_path(cache).is_file():
        return []
    found: list[Source] = []
    hub = load(checkout)
    if hub is not None:
        found += [State(hub), Records(hub, local), Metrics(hub, local)]
    daemon = ignite_home() / "daemon.db"
    if daemon.is_file():
        found += [DaemonPasses(daemon, local), DaemonMetrics(daemon, local)]
    return found
