"""Enact's ownership lock (docs/efficiency-dashboard.md, "Time accounting"): how long sessions
waited for it and how long its remote syncs took, read from Enact's lock-wait log and joined to
the tool calls that ran each command.

Enact writes `<main checkout>/.git/enact/lock-waits.jsonl`, renamed to `lock-waits.1.jsonl`
when it reaches 8 MB, with one line per acquisition of its ownership lock, written when the
lock is released or the wait gives up (~/enact/docs/design.md, "Lock-wait log"):

- `time`: when the wait began, in UTC to the second;
- `pid` and `command`, the first four words of the command line, such as
  `/path/enact.py meta by-x enact_session=...`;
- `outcome`, `acquired` or `gave_up`;
- `waited_s`, and for an acquired lock `held_s` and `sync_s`, the seconds spent in remote
  syncs while holding it (part of `held_s`), with `syncs` their count;
- `holder`: the command holding the lock once the wait passed a second, else null.

One command can take the lock several times; each line is joined on its own to the top-level
tool call that ran the command, and `timing.py` moves the joined lines' waited and sync seconds
out of the call's kind into `enact lock wait` and `enact sync`. Lines from commands other than
`enact.py`, such as Enact's own unit tests, are not read.

The ticket-server merge queue writes `staging-lease-waits.jsonl` and rotates it in the same
directory. Each record gives the first busy reply's UTC time, the push's pid and ticket,
the outcome and waited_s. The time split records these seconds as `enact lease wait`.
"""

from __future__ import annotations

import json
import re
import sqlite3
from collections import defaultdict
from dataclasses import asdict, dataclass
from datetime import timedelta
from pathlib import Path

from . import claude, kinds, ticket_ids, timing
from .timing import LOCK_WAIT, SYNC
from .cpu import GIVE_UP, LONGEST_CALL, SLACK, iso, parse
from .facts import Facts
from .finding import bounds, wanted
from .ingest import LineSource, Row
from .render import Result, describe, table, wrap
from .scope import Scope

# How a holder's command reads when the wait was too short to look.
UNSEEN = "(not seen)"
STAGING_LOG = "staging-lease-waits"


def folder(checkout: Path) -> Path:
    """Where Enact keeps the main checkout's state, its lock and the lock-wait log."""
    return checkout / ".git" / "enact"


def words(command: str) -> tuple[str, str | None] | None:
    """The Enact subcommand a lock line's command ran and the first ticket it names; None for
    a command that is not `enact.py`."""
    parts = command.split()
    for number, part in enumerate(parts):
        if part.endswith("enact.py") and number + 1 < len(parts):
            arguments = parts[number + 2 :]
            # Prefer a prefixed ID anywhere, as legacy lock records can have flags
            # before it. A bare ID is unambiguous as the first argument or after --bead.
            bead = next(
                (w for w in arguments if re.fullmatch(ticket_ids.LEGACY, w)), None
            )
            if bead is None and arguments:
                candidate = arguments[0]
                if "--bead" in arguments and arguments.index("--bead") + 1 < len(
                    arguments
                ):
                    candidate = arguments[arguments.index("--bead") + 1]
                if ticket_ids.WORKTREE_TICKET.fullmatch(candidate):
                    bead = candidate
            return parts[number + 1], bead
    return None


def subcommand(command: str | None) -> str:
    """How a holder's command reads in a breakdown: `enact <subcommand>`."""
    if not command:
        return UNSEEN
    found = words(command)
    return f"enact {found[0]}" if found else Path(command.split()[0]).name


# A shell function or variable defined in a command, such as `E() {` or `E=`.
DEFINED: re.Pattern[str] = re.compile(r"\b([A-Za-z_]\w*)(?:\s*\(\)\s*\{|=)")


def runs(command: str, sub: str) -> bool:
    """Whether a shell command runs the Enact subcommand `sub`: called by its path or as
    `enact`, through a variable such as `$E`, or through a shell function such as `E`
    defined earlier in the same command."""
    if "enact" not in command:
        return False
    names = "|".join(re.escape(n) for n in DEFINED.findall(command))
    caller = rf"\benact(?:\.py|\.sh)?|\$\{{?\w+\}}?" + (
        rf"|(?:^|[;&|(\s])(?:{names})" if names else ""
    )
    pattern = rf"(?:{caller})\"?\s+{re.escape(sub)}(?:\s|$|\")"
    return re.search(pattern, command) is not None


class LockLog(LineSource):
    """Enact's lock-wait log, and the one it was rotated to."""

    name = "lock_waits"

    def __init__(self, folder: Path, host: str) -> None:
        self.folder = folder
        self.host = host

    def files(self) -> list[Path]:
        names = [
            stem + suffix
            for stem in ("lock-waits", STAGING_LOG)
            for suffix in (".1.jsonl", ".jsonl")
        ]
        return [p for p in (self.folder / n for n in names) if p.exists()]

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        staging = path.name.startswith(STAGING_LOG)
        if staging:
            command = "enact.py push"
            found = ("push", record["bead"])
        else:
            command = str(record["command"])
            found = words(command)
            if found is None:
                return []
        acquired = not staging and str(record["outcome"]) == "acquired"
        holder = record.get("holder")
        return [
            (
                "lock_waits",
                {
                    "host": self.host,
                    "time": iso(parse(str(record["time"]))),
                    "pid": int(record["pid"]),
                    "command": command,
                    "subcommand": found[0],
                    "bead": found[1],
                    "outcome": str(record["outcome"]),
                    "waited_seconds": float(record["waited_s"]),
                    "held_seconds": float(record["held_s"]) if acquired else 0.0,
                    "sync_seconds": float(record["sync_s"]) if acquired else 0.0,
                    "syncs": int(record["syncs"]) if acquired else 0,
                    "holder": str(holder) if holder else None,
                    "staging_lease": int(staging),
                },
            )
        ]

    def finish(self, db: sqlite3.Connection, changed: list[Path]) -> int:
        joined = join(db, self.host)
        table_, _ = kinds.load()
        heavy = frozenset(k.kind for k in table_ if k.heavy)
        return sum(
            timing.account(db, self.host, provider, transcript, heavy)
            for provider, transcript in sorted(joined)
        )


def join(db: sqlite3.Connection, host: str) -> set[tuple[str, str]]:
    """Joins each line of `host` not joined yet to the top-level tool call that was running
    for the whole of its wait and hold and whose command runs its subcommand, or whose
    command mentions Enact but was cut to `claude.COMMAND_CHARACTERS` before the store kept
    it (a later part of a long command may have run it). One whose command shows the
    subcommand wins, then one that names its ticket, then the latest to start. A line with no such call is tried again at the
    next ingest, until the store's latest tool call on the host is `GIVE_UP` later. Returns
    the transcripts of the calls joined, as (provider, transcript)."""
    pending = db.execute(
        "SELECT rowid, time, subcommand, bead, waited_seconds + held_seconds, staging_lease "
        "FROM lock_waits WHERE host = ? AND call IS NULL",
        (host,),
    ).fetchall()
    if not pending:
        return set()
    latest = db.execute(
        "SELECT max(start) FROM tool_calls WHERE host = ?", (host,)
    ).fetchone()[0]
    joined: set[tuple[str, str]] = set()
    for rowid, time, sub, bead, length, staging in pending:
        start = parse(str(time))
        end = start + timedelta(seconds=float(length))
        ticket = ticket_ids.stored(db, str(bead)) if bead is not None else None
        found = None
        best = (False, False, "")
        for provider, transcript, call, call_start, command, owner in db.execute(
            """SELECT c.provider, c.transcript, c.call, c.start, c.command, c.bead
            FROM tool_calls AS c JOIN transcripts AS t ON t.host = c.host
                AND t.provider = c.provider AND t.transcript = c.transcript
            WHERE c.host = ? AND t.parent IS NULL AND c.command LIKE '%enact%'
                AND c.start >= ? AND c.start <= ? AND c.end >= ?""",
            (
                host,
                iso(start - LONGEST_CALL),
                iso(start + timedelta(seconds=SLACK)),
                iso(end - timedelta(seconds=SLACK)),
            ),
        ):
            shown = runs(str(command), str(sub))
            if not shown and len(str(command)) < claude.COMMAND_CHARACTERS:
                continue
            call_ticket = (
                ticket_ids.stored(db, str(owner)) if owner is not None else None
            )
            # The batch's other pushes can overlap this wait. Only the integrator waits
            # for the server lease, and its own ticket is in the log.
            if staging and ticket != call_ticket:
                continue
            named = bead is not None and (
                str(bead) in str(command) or ticket == call_ticket
            )
            rank = (shown, named, str(call_start))
            if found is None or rank > best:
                found, best = (str(provider), str(transcript), str(call)), rank
        if found is not None:
            db.execute(
                "UPDATE lock_waits SET provider = ?, transcript = ?, call = ? "
                "WHERE rowid = ?",
                (*found, rowid),
            )
            joined.add((found[0], found[1]))
        elif latest is not None and parse(str(latest)) - end > GIVE_UP:
            db.execute("UPDATE lock_waits SET call = '' WHERE rowid = ?", (rowid,))
    return joined


@dataclass
class Subcommand:
    """One Enact subcommand's lock lines over the range. `calls` and `call_minutes` are
    the tool calls its lines were joined to, and their wall minutes."""

    subcommand: str
    lines: int
    acquisitions: int
    gave_up: int
    wait_minutes: float
    sync_minutes: float
    held_minutes: float
    calls: int
    call_minutes: float


@dataclass
class Holder:
    """A command seen holding the lock, and the waits it held up."""

    holder: str
    waits: int
    wait_minutes: float


@dataclass
class Report:
    since: str | None
    range: str
    filters: str
    lock_wait_minutes: float
    sync_minutes: float
    lines: int
    joined: int
    unjoined: int
    pending: int
    subcommands: list[Subcommand]
    holders: list[Holder]


def minutes(seconds: float) -> float:
    return round(seconds / 60, 1)


def in_range(scope: Scope) -> tuple[str, list[object]]:
    """A `WHERE` clause (starting with `AND`) keeping the lines `l` in the scope's range and
    on its host: the provider is the joined call's, so it is checked apart."""
    clause, given = bounds("l.time", scope, provider=False)
    return clause.replace(" AND host = ", " AND l.host = "), given


def held_up(
    db: sqlite3.Connection, facts: Facts, scope: Scope
) -> list[tuple[str, str | None, float]]:
    """The waits of the scope's lines joined to a tool call in its cohort, as (holder, the
    call's ticket, waited seconds): only joined lines count, since only they are agent time.
    """
    clause, given = in_range(scope)
    keep = wanted(facts, scope)
    found: list[tuple[str, str | None, float]] = []
    for holder, bead, waited, provider in db.execute(
        f"""SELECT l.holder, c.bead, l.waited_seconds, c.provider FROM lock_waits AS l
        JOIN tool_calls AS c ON c.host = l.host AND c.provider = l.provider
            AND c.transcript = l.transcript AND c.call = l.call
        WHERE l.staging_lease = 0 AND l.call != '' AND l.waited_seconds > 0{clause}""",
        given,
    ):
        if scope.provider and str(provider) != scope.provider:
            continue
        if keep(str(bead) if bead else None):
            found.append((subcommand(holder), str(bead) if bead else None, waited))
    return found


def report(db: sqlite3.Connection, facts: Facts, scope: Scope) -> Result:
    """Agent minutes waiting for Enact's lock and syncing under it, as `time --by kind`
    counts them, then the log's lines by subcommand and the commands that held the lock
    while others waited. The breakdowns read every line on the scope's hosts, joined or
    not, since a command run outside a session (such as a vizier's loop) also holds others
    up; cohort filters keep only the lines joined to the cohort's calls."""
    agent: dict[str, float] = defaultdict(float)
    for w in facts.chosen(scope):
        for (category, kind, _), value in w.time.items():
            if category == "tool" and kind in (LOCK_WAIT, SYNC):
                agent[kind] += value
    clause, given = in_range(scope)
    keep = wanted(facts, scope)
    cohort = facts.cohort(scope) is not None or scope.provider is not None
    by_sub: dict[str, Subcommand] = {}
    calls: dict[str, dict[tuple[str, str, str], float]] = defaultdict(dict)
    holders: dict[str, Holder] = {}
    counts = {"lines": 0, "joined": 0, "unjoined": 0, "pending": 0}
    for row in db.execute(
        f"""SELECT l.subcommand, l.outcome, l.waited_seconds, l.held_seconds,
            l.sync_seconds, l.holder, l.call, c.provider, c.transcript, c.start, c.end,
            c.bead
        FROM lock_waits AS l LEFT JOIN tool_calls AS c ON c.host = l.host
            AND c.provider = l.provider AND c.transcript = l.transcript
            AND c.call = l.call
        WHERE l.staging_lease = 0{clause}""",
        given,
    ):
        sub, outcome, waited, held, synced, holder, call = row[:7]
        joined = call is not None and call != "" and row[9] is not None
        if cohort and not (
            joined
            and (scope.provider is None or str(row[7]) == scope.provider)
            and keep(str(row[11]) if row[11] else None)
        ):
            continue
        counts["lines"] += 1
        counts["joined" if joined else "pending" if call is None else "unjoined"] += 1
        name = f"enact {sub}"
        entry = by_sub.setdefault(
            name, Subcommand(name, 0, 0, 0, 0.0, 0.0, 0.0, 0, 0.0)
        )
        entry.lines += 1
        entry.acquisitions += int(outcome == "acquired")
        entry.gave_up += int(outcome == "gave_up")
        entry.wait_minutes += float(waited)
        entry.sync_minutes += float(synced)
        entry.held_minutes += float(held)
        if joined:
            span = timing.seconds(str(row[10])) - timing.seconds(str(row[9]))
            calls[name][(str(row[7]), str(row[8]), str(call))] = span
        if float(waited) > 0:
            seen = subcommand(holder)
            found = holders.setdefault(seen, Holder(seen, 0, 0.0))
            found.waits += 1
            found.wait_minutes += float(waited)
    subs = []
    for entry in by_sub.values():
        entry.wait_minutes = minutes(entry.wait_minutes)
        entry.sync_minutes = minutes(entry.sync_minutes)
        entry.held_minutes = minutes(entry.held_minutes)
        entry.calls = len(calls[entry.subcommand])
        entry.call_minutes = minutes(sum(calls[entry.subcommand].values()))
        subs.append(entry)
    subs.sort(
        key=lambda s: (
            -(s.wait_minutes + s.sync_minutes),
            -s.held_minutes,
            s.subcommand,
        )
    )
    ranked = sorted(holders.values(), key=lambda h: (-h.wait_minutes, h.holder))
    for h in ranked:
        h.wait_minutes = minutes(h.wait_minutes)
    result = Report(
        scope.range.since,
        scope.range.name,
        scope.describe(),
        minutes(agent[LOCK_WAIT]),
        minutes(agent[SYNC]),
        counts["lines"],
        counts["joined"],
        counts["unjoined"],
        counts["pending"],
        subs,
        ranked,
    )
    flags = scope.flags()
    return Result(
        "enact",
        asdict(result),
        render(result),
        [
            "efficiency time --by kind " + flags,
            "efficiency findings --rule enact-lock-wait " + flags,
            "efficiency sql 'select * from lock_waits where staging_lease = 0 order by time desc limit 20'",
        ],
        describe("since", "lock_wait_minutes", "sync_minutes", "lock_lines"),
    )


def render(r: Report) -> list[str]:
    lines = [
        f"enact lock   {r.range}" + (f"  {r.filters}" if r.filters else ""),
        f"agent min    lock wait {r.lock_wait_minutes:.1f} | sync {r.sync_minutes:.1f}",
        *wrap(
            "lines        ",
            [
                f"{r.lines} lines",
                f"joined {r.joined}",
                f"no call {r.unjoined}",
                f"not joined yet {r.pending}",
            ],
            13,
        ),
    ]
    if r.subcommands:
        lines += [
            "",
            *table(
                [
                    "subcommand",
                    "lines",
                    "gave up",
                    "wait min",
                    "sync min",
                    "held min",
                    "calls",
                    "call min",
                ],
                [
                    [
                        s.subcommand,
                        s.lines,
                        s.gave_up,
                        s.wait_minutes,
                        s.sync_minutes,
                        s.held_minutes,
                        s.calls,
                        s.call_minutes,
                    ]
                    for s in r.subcommands
                ],
            ),
        ]
    if r.holders:
        lines += [
            "",
            "held the lock while others waited",
            *table(
                ["holder", "waits", "wait min"],
                [[h.holder, h.waits, h.wait_minutes] for h in r.holders],
            ),
        ]
    return lines
