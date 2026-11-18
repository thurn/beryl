"""CPU accounting (docs/efficiency-dashboard.md, "CPU accounting"): which tool call a lease's
wait belongs to, and which ticket each minute of a worktree's CPU belongs to. The `cpu`,
`cpu_minutes` and `leases` rows themselves come from Ignite's records (`ignite.py`); rows
from before Ignite, written by the old CPU sampler and CPU token pool, stay in the store.

A lease not yet joined is joined to the tool call that was running in the lease's tree while
it waited, and the transcripts of those calls are accounted again, so that the seconds move
from the call's kind to "queued for cores" (`timing.py`).
"""

from __future__ import annotations

import sqlite3
from collections import defaultdict
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

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
    """The last part of a worktree's path, which is how the sampler, the token pool and
    Ignite's records name it; empty for none."""
    return Path(str(path)).name if path else ""


def call_worktrees(
    db: sqlite3.Connection, host: str
) -> tuple[dict[str, str], dict[tuple[str, str], list[tuple[str, str, str]]]]:
    """Each ticket's worktree, and each transcript's windows on no ticket that name the worktree
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
    """A ticket that may own CPU in its worktree: when it started and closed (`None` while
    open), and its windows as (start, end), with `None` for a window still open."""

    bead: str
    host: str | None
    started: str | None
    closed: str | None
    windows: tuple[tuple[str, str | None], ...]


def owner_of(claims: Iterable[Claim], moment: str) -> str | None:
    """The ticket a minute of its worktree's CPU belongs to: the ticket with a window open at
    that minute, or else the one whose start and close contain it, the latest started if
    several do; `None` when no ticket claims it, which makes it shared."""
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
    """A worktree's CPU seconds by the ticket each minute belongs to, and the seconds no ticket
    claims. A ticket that names the host it ran on claims only that host's minutes."""
    by_bead: dict[str, float] = defaultdict(float)
    shared = 0.0
    for host, moment, seconds in minutes:
        bead = owner_of((c for c in claims if c.host is None or c.host == host), moment)
        if bead is None:
            shared += seconds
        else:
            by_bead[bead] += seconds
    return dict(by_bead), shared
