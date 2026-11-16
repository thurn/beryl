"""CPU accounting (docs/efficiency-dashboard.md, "CPU accounting"): the CPU sampler's rows, the
CPU token pool's log of leases, and which bead each minute of a worktree's CPU belongs to.

Both files are written by the repository's tools and read here by their documented formats:

- `<cache>/cpu/samples.jsonl` (tools/src/beryl_tools/cpu.py) has one row per minute: `time`
  (seconds since the epoch), `seconds`, `cores`, `cpu` (CPU seconds keyed by
  `owner/kind/worktree`, with `-` for no worktree) and `peak_cores`. The sampler calls the
  owner the context. Each row becomes a `cpu_minutes` row and one `cpu` row per key.
- `<cache>/cpu-tokens/leases.jsonl` (tools/src/beryl_tools/tokens.py) has one line per lease,
  written when it is released: `time`, `class`, `worktree`, `command`, `cores`, `wanted`,
  `waited_s` and `held_s`. A lease waited from `time - held_s - waited_s` to `time - held_s`.
  A wait that got no lease has a line too, with `cores` 0, `held_s` 0 and an `outcome`
  (`gave_up`, or `abandoned` when its process died waiting).

After each ingest the leases not yet joined are joined to the tool call that was running in
the lease's worktree while it waited, and the transcripts of those calls are accounted again,
so that the seconds move from the call's kind to "queued for cores" (`timing.py`).
"""

from __future__ import annotations

import json
import sqlite3
from collections import defaultdict
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import kinds, timing
from .ingest import LineSource, Row

# How close, in seconds, a lease's wait must fall inside a tool call to be joined to it: the
# log's times are rounded to the second and its waits to a tenth.
SLACK = 2.0
# How long before a lease's wait a tool call that was still running then may have started.
LONGEST_CALL = timedelta(hours=12)
# A lease no call was found for is given up on once the store holds tool calls this much
# later than it: its session's transcript would have been read by then.
GIVE_UP = timedelta(days=1)


def iso(moment: datetime) -> str:
    return moment.isoformat(timespec="milliseconds").replace("+00:00", "Z")


def from_epoch(value: float) -> str:
    return iso(datetime.fromtimestamp(value, timezone.utc))


def parse(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).replace(
        tzinfo=timezone.utc
    )


def worktree_name(path: object) -> str:
    """The last part of a worktree's path, which is how the sampler and the token pool name
    it; empty for none."""
    return Path(str(path)).name if path else ""


class Samples(LineSource):
    """The CPU sampler's rows."""

    name = "cpu"

    def __init__(self, path: Path, host: str) -> None:
        self.path = path
        self.host = host

    def files(self) -> list[Path]:
        return [self.path] if self.path.exists() else []

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        time = from_epoch(float(record["time"]))
        seconds = float(record["seconds"])
        used: Mapping[str, object] = record["cpu"]
        rows: list[Row] = [
            (
                "cpu_minutes",
                {
                    "host": self.host,
                    "time": time,
                    "seconds": seconds,
                    "cores": int(record["cores"]),
                    "cpu_seconds": sum(float(str(v)) for v in used.values()),
                    "peak_cores": float(record.get("peak_cores") or 0),
                },
            )
        ]
        for key, value in used.items():
            owner, kind, worktree = key.split("/", 2)
            rows.append(
                (
                    "cpu",
                    {
                        "host": self.host,
                        "time": time,
                        "owner": owner,
                        "kind": kind,
                        "worktree": "" if worktree == "-" else worktree,
                        "seconds": seconds,
                        "cpu_seconds": float(str(value)),
                    },
                )
            )
        return rows


class Leases(LineSource):
    """The CPU token pool's log of leases."""

    name = "leases"

    def __init__(self, path: Path, host: str) -> None:
        self.path = path
        self.host = host

    def files(self) -> list[Path]:
        return [self.path] if self.path.exists() else []

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        released = parse(str(record["time"]))
        waited = float(record["waited_s"])
        held = float(record["held_s"])
        wait_end = released - timedelta(seconds=held)
        return [
            (
                "leases",
                {
                    "host": self.host,
                    "time": iso(released),
                    "class": str(record["class"]),
                    "worktree": str(record["worktree"]),
                    "command": str(record["command"]),
                    "cores": int(record["cores"]),
                    "wanted": int(record["wanted"]),
                    "waited_seconds": waited,
                    "held_seconds": held,
                    "wait_start": iso(wait_end - timedelta(seconds=waited)),
                    "wait_end": iso(wait_end),
                },
            )
        ]

    def finish(self, db: sqlite3.Connection, changed: list[Path]) -> int:
        joined = join(db, self.host)
        table, _ = kinds.load()
        heavy = frozenset(k.kind for k in table if k.heavy)
        return sum(
            timing.account(db, self.host, provider, transcript, heavy)
            for provider, transcript in sorted(joined)
        )


def call_worktrees(
    db: sqlite3.Connection, host: str
) -> tuple[dict[str, str], dict[tuple[str, str], list[tuple[str, str, str]]]]:
    """Each bead's worktree, and each transcript's windows on no bead that name the worktree
    the session made, as (start, end, worktree)."""
    beads = {
        str(r[0]): worktree_name(r[1])
        for r in db.execute(
            "SELECT id, json_extract(enact, '$.enact_worktree') FROM beads"
        )
        if r[1]
    }
    labelled: dict[tuple[str, str], list[tuple[str, str, str]]] = defaultdict(list)
    for provider, transcript, start, end, label in db.execute(
        "SELECT provider, transcript, start, coalesce(end, '9999'), label FROM windows "
        "WHERE host = ? AND kind = 'none' AND label IS NOT NULL",
        (host,),
    ):
        labelled[(str(provider), str(transcript))].append(
            (str(start), str(end), worktree_name(label))
        )
    return beads, labelled


def join(db: sqlite3.Connection, host: str) -> set[tuple[str, str]]:
    """Joins each lease of `host` not joined yet that waited at all to the top-level tool call
    running in its worktree while it waited; the latest to start wins when several were.
    A lease with no such call is tried again at the next ingest, until the store's latest
    tool call on the host is `GIVE_UP` later. Returns the transcripts of the calls joined, as (provider, transcript).
    """
    pending = db.execute(
        "SELECT rowid, time, worktree, wait_start, wait_end, waited_seconds FROM leases "
        "WHERE host = ? AND call IS NULL",
        (host,),
    ).fetchall()
    if not pending:
        return set()
    latest = db.execute(
        "SELECT max(start) FROM tool_calls WHERE host = ?", (host,)
    ).fetchone()[0]
    beads, labelled = call_worktrees(db, host)
    joined: set[tuple[str, str]] = set()
    for rowid, time, worktree, wait_start, wait_end, waited in pending:
        found = None
        if float(waited) > 0:
            start, end = parse(str(wait_start)), parse(str(wait_end))
            for provider, transcript, call, call_start, bead in db.execute(
                """SELECT c.provider, c.transcript, c.call, c.start, c.bead
                FROM tool_calls AS c JOIN transcripts AS t ON t.host = c.host
                    AND t.provider = c.provider AND t.transcript = c.transcript
                WHERE c.host = ? AND t.parent IS NULL AND c.start >= ? AND c.start <= ?
                    AND c.end >= ? ORDER BY c.start DESC""",
                (
                    host,
                    iso(start - LONGEST_CALL),
                    iso(start + timedelta(seconds=SLACK)),
                    iso(end - timedelta(seconds=SLACK)),
                ),
            ):
                key = (str(provider), str(transcript))
                there = beads.get(str(bead)) if bead else None
                if there is None:
                    there = next(
                        (
                            w
                            for s, e, w in labelled.get(key, [])
                            if s <= str(call_start) < e
                        ),
                        None,
                    )
                if there == str(worktree):
                    found = (*key, str(call))
                    break
        if found is not None:
            db.execute(
                "UPDATE leases SET provider = ?, transcript = ?, call = ? WHERE rowid = ?",
                (*found, rowid),
            )
            joined.add((found[0], found[1]))
        elif float(waited) <= 0 or (
            latest is not None and parse(str(latest)) - parse(str(time)) > GIVE_UP
        ):
            db.execute("UPDATE leases SET call = '' WHERE rowid = ?", (rowid,))
    return joined


@dataclass(frozen=True)
class Claim:
    """A bead that may own CPU in its worktree: when it started and closed (`None` while
    open), and its windows as (start, end), with `None` for a window still open."""

    bead: str
    host: str | None
    started: str | None
    closed: str | None
    windows: tuple[tuple[str, str | None], ...]


def owner_of(claims: Iterable[Claim], moment: str) -> str | None:
    """The bead a minute of its worktree's CPU belongs to: the bead with a window open at
    that minute, or else the one whose start and close contain it, the latest started if
    several do; `None` when no bead claims it, which makes it shared."""
    found = list(claims)
    for c in found:
        if any(s <= moment and (e is None or moment < e) for s, e in c.windows):
            return c.bead
    open_then = [
        c
        for c in found
        if c.started is not None
        and c.started <= moment
        and (c.closed is None or moment <= c.closed)
    ]
    return max(open_then, key=lambda c: str(c.started)).bead if open_then else None


def worktree_minutes(
    db: sqlite3.Connection,
    worktree: str,
    since: str | None,
    until: str | None,
    host: str | None,
) -> list[tuple[str, str, float]]:
    """The CPU seconds of each minute in `worktree`, as (host, time, seconds)."""
    clause = ""
    given: list[object] = [worktree]
    for column, op, value in (
        ("time", ">=", since),
        ("time", "<", until),
        ("host", "=", host),
    ):
        if value is not None:
            clause += f" AND {column} {op} ?"
            given.append(value)
    return [
        (str(r[0]), str(r[1]), float(r[2]))
        for r in db.execute(
            f"SELECT host, time, sum(cpu_seconds) FROM cpu WHERE worktree = ?{clause} "
            "GROUP BY host, time",
            given,
        )
    ]


def attribute(
    minutes: Iterable[tuple[str, str, float]], claims: list[Claim]
) -> tuple[dict[str, float], float]:
    """A worktree's CPU seconds by the bead each minute belongs to, and the seconds no bead
    claims. A bead that names the host it ran on claims only that host's minutes."""
    by_bead: dict[str, float] = defaultdict(float)
    shared = 0.0
    for host, moment, seconds in minutes:
        bead = owner_of((c for c in claims if c.host is None or c.host == host), moment)
        if bead is None:
            shared += seconds
        else:
            by_bead[bead] += seconds
    return dict(by_bead), shared


def sampler_cpu() -> tuple[float, float] | None:
    """The CPU sampler's own CPU seconds and how long it has run, from its launchd agent's
    process (`by cpu install`) and every process under it (the agent runs `uv`, which runs
    the sampler), with the children each has reaped (`ps -S`); `None` when it is not running
    or this is not a Mac."""
    import subprocess

    try:
        listed = subprocess.run(
            ["launchctl", "list", "ca.thurn.beryl.cpu"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
        table = subprocess.run(
            ["ps", "-A", "-S", "-o", "pid=,ppid=,cputime=,etime="],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
    except OSError:
        return None
    pid = next(
        (
            line.split("=")[1].strip(" ;")
            for line in listed.splitlines()
            if '"PID"' in line
        ),
        None,
    )
    return tree_cpu(table, pid) if pid else None


def tree_cpu(table: str, top: str) -> tuple[float, float] | None:
    """From `ps -o pid=,ppid=,cputime=,etime=` output, the CPU seconds of `top` and every
    process under it, and how long `top` has run; `None` when `top` is not listed."""
    rows = [line.split() for line in table.splitlines()]
    rows = [r for r in rows if len(r) == 4]
    children: dict[str, list[str]] = defaultdict(list)
    for pid, parent, _, _ in rows:
        children[parent].append(pid)
    times = {r[0]: (clock_seconds(r[2]), clock_seconds(r[3])) for r in rows}
    if top not in times:
        return None
    used = 0.0
    stack = [top]
    while stack:
        pid = stack.pop()
        used += times[pid][0]
        stack.extend(children.get(pid, []))
    return used, times[top][1]


def clock_seconds(text: str) -> float:
    """Seconds in `ps`'s time columns: `1:02.50`, `01:02:03` or `2-01:02:03`."""
    days, _, rest = text.rpartition("-")
    total = 0.0
    for part in rest.split(":"):
        total = total * 60 + float(part)
    return total + (int(days) * 86400 if days else 0)


def samples_path(cache: Path) -> Path:
    return cache / "cpu" / "samples.jsonl"


def leases_path(cache: Path) -> Path:
    return cache / "cpu-tokens" / "leases.jsonl"
