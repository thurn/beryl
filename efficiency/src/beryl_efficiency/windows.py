"""Bead windows: which bead each request and tool call of a session belongs to
(docs/efficiency-dashboard.md, "Attributing sessions to beads").

A window opens at a session's `enact start <bead>` or `enact claim <bead>`, and at the result
of `enact close --continue` for the bead it claimed. It closes at the result of `enact close
<bead>`, at the opening of another bead's window, or at the end of the transcript. When the
result of `enact close --continue` names no bead (its output was cut short), the next bead the
session closes or creates a worktree for opens at that close. Rows before the first window
belong to it. Stretches outside every bead window are windows of kind `none`, so a top-level
transcript's windows cover it from its first record to its last. A transcript that opens no
window is one window: `orchestration`, charged to its epic, when beads name it as their
vizier, otherwise `none`. A subagent's rows all belong to the parent's window that holds the
subagent's first record.

Windows are derived again, whole, for each transcript that gained lines in an ingest.
"""

from __future__ import annotations

import re
import sqlite3
from collections import Counter
from dataclasses import dataclass

Connection = sqlite3.Connection
# A bead id, as in `claude.BEAD`; a worktree named like one names the bead it is for.
BEAD_ID: re.Pattern[str] = re.compile(r"[a-z][a-z0-9]*-[a-z0-9]+(?:\.[0-9]+)*")


@dataclass
class Window:
    start: str
    end: str
    bead: str | None
    kind: str
    label: str | None = None


@dataclass
class Event:
    time: str
    call: str
    verb: str
    bead: str


def bead_spans(events: list[Event], last: str) -> tuple[list[Window], int]:
    """The bead windows that `events` open and close, in order, and how many closes named a
    bead with no open window."""
    closing = {e.call for e in events if e.verb == "close"}
    continuing = {e.call for e in events if e.verb == "continue"}
    # `enact close --continue` closes its bead where the next one opens, at its result, so
    # that the two windows meet with no gap. A "next" bead printed by any other command is
    # not a claim.
    claimed = {e.call: e.time for e in events if e.verb == "next" and e.call in closing}
    ordered = sorted(
        (
            Event(claimed.get(e.call, e.time), e.call, e.verb, e.bead)
            for e in events
            if e.verb in ("open", "close", "worktree")
            or (e.verb == "next" and e.call in closing)
        ),
        key=lambda e: (e.time, e.verb != "close"),
    )
    spans: list[Window] = []
    current: Window | None = None
    # When a continued close's result named no bead (its output was cut short, say), the
    # time of that close: the next bead the session names opens there.
    unclaimed: str | None = None
    stray = 0
    for event in ordered:
        named = event.verb == "close" or (
            event.verb == "worktree" and BEAD_ID.fullmatch(event.bead) is not None
        )
        if current is None and unclaimed is not None and named:
            current = Window(unclaimed, last, event.bead, "bead")
        if event.verb in ("open", "next"):
            unclaimed = None
            if current is not None and current.bead == event.bead:
                continue
            if current is not None:
                current.end = event.time
                spans.append(current)
            current = Window(event.time, last, event.bead, "bead")
        elif event.verb == "close":
            unclaimed = None
            if current is not None and current.bead == event.bead:
                current.end = event.time
                spans.append(current)
                current = None
                if event.call in continuing and event.call not in claimed:
                    unclaimed = event.time
            elif not spans or spans[-1].bead != event.bead or current is not None:
                stray += 1  # A repeated close of the bead just closed is not stray.
    if current is not None:
        spans.append(current)
    return spans, stray


def cover(spans: list[Window], first: str, last: str, whole: Window) -> list[Window]:
    """`spans` stretched to cover `first` to `last`: the first starts at `first`, and gaps
    become `none` windows labelled like `whole`. With no spans, `whole` alone."""
    if not spans:
        return [whole]
    spans[0].start = min(first, spans[0].start)
    covered: list[Window] = []
    reached = spans[0].start
    for span in spans:
        if span.start > reached:
            covered.append(Window(reached, span.start, None, "none", whole.label))
        covered.append(span)
        reached = max(reached, span.end)
    if reached < last:
        covered.append(Window(reached, last, None, "none", whole.label))
    return covered


def bounds(
    db: Connection, host: str, provider: str, transcript: str
) -> tuple[str | None, str | None]:
    """The time of a transcript's first record and of its last."""
    found = db.execute(
        """SELECT min(t), max(t) FROM (
            SELECT time AS t FROM requests WHERE host = ?1 AND provider = ?2
                AND transcript = ?3
            UNION ALL SELECT start FROM tool_calls WHERE host = ?1 AND provider = ?2
                AND transcript = ?3
            UNION ALL SELECT end FROM tool_results WHERE host = ?1 AND provider = ?2
                AND transcript = ?3)""",
        (host, provider, transcript),
    ).fetchone()
    return (found[0], found[1]) if found else (None, None)


def session_of(db: Connection, host: str, provider: str, transcript: str) -> str:
    found = db.execute(
        "SELECT session FROM sessions WHERE host = ? AND provider = ? AND transcript = ?",
        (host, provider, transcript),
    ).fetchone()
    return str(found[0]) if found else transcript


def orchestrated_epic(db: Connection, transcript: str, session: str) -> str | None:
    """The epic of the beads that name this session or transcript as their vizier: each one's
    run epic, else its parent, whichever is most common."""
    epics = Counter(
        str(r[0])
        for r in db.execute(
            """SELECT coalesce(json_extract(enact, '$.enact_run'), parent) FROM beads
            WHERE json_extract(enact, '$.enact_vizier') IN (?, ?)""",
            (transcript, session),
        )
        if r[0]
    )
    return epics.most_common(1)[0][0] if epics else None


def windows_of(
    db: Connection, host: str, provider: str, transcript: str, session: str
) -> tuple[list[Window], int]:
    first, last = bounds(db, host, provider, transcript)
    if first is None or last is None:
        return [], 0
    # A close ends its window when the command returns, so the request that ran it, and the
    # bead's own close, belong to the bead.
    rows = db.execute(
        """SELECT CASE WHEN e.verb = 'close' THEN coalesce(r.end, e.time) ELSE e.time END,
            e.call, e.verb, e.bead
        FROM enact_events AS e LEFT JOIN tool_results AS r ON r.host = e.host
            AND r.provider = e.provider AND r.transcript = e.transcript AND r.call = e.call
        WHERE e.host = ? AND e.provider = ? AND e.transcript = ?""",
        (host, provider, transcript),
    ).fetchall()
    events = [Event(str(r[0]), str(r[1]), str(r[2]), str(r[3])) for r in rows]
    worktrees = sorted((e.time, e.bead) for e in events if e.verb == "worktree")
    label = worktrees[0][1] if worktrees else None
    spans, stray = bead_spans(events, last)
    epic = None if spans else orchestrated_epic(db, transcript, session)
    whole = (
        Window(first, last, epic, "orchestration")
        if epic
        else Window(first, last, None, "none", label)
    )
    return cover(spans, first, last, whole), stray


def attribute(
    db: Connection,
    host: str,
    provider: str,
    transcript: str,
    found: list[Window],
    subagents: list[str],
) -> None:
    """Sets `bead` on the rows of `transcript` from its windows, the first window taking every
    row before it and the last every row after it, and on each subagent's rows from the
    window that holds the subagent's first record."""
    key = (host, provider, transcript)
    for index, window in enumerate(found):
        low = window.start if index else ""
        high = found[index + 1].start if index + 1 < len(found) else "\U0010ffff"
        for table, column in (("requests", "time"), ("tool_calls", "start")):
            db.execute(
                f"UPDATE {table} SET bead = ? WHERE host = ? AND provider = ? "
                f"AND transcript = ? AND {column} >= ? AND {column} < ?",
                (window.bead, *key, low, high),
            )
    for subagent in subagents:
        first, _ = bounds(db, host, provider, subagent)
        holding = [w for w in found if first is not None and w.start <= first]
        bead = (holding[-1] if holding else found[0]).bead if found else None
        for table in ("requests", "tool_calls"):
            db.execute(
                f"UPDATE {table} SET bead = ? WHERE host = ? AND provider = ? "
                "AND transcript = ?",
                (bead, host, provider, subagent),
            )


def derive(
    db: Connection,
    host: str,
    provider: str,
    changed: list[tuple[str, str | None]],
) -> int:
    """Derives again the transcripts rows, tool call ends, windows and attribution for the
    transcripts `changed`, given as `(id, parent)` with `parent` set for a subagent. Returns
    how many windows it wrote."""
    tops: set[str] = set()
    for transcript, parent in changed:
        top = parent or transcript
        tops.add(top)
        first, last = bounds(db, host, provider, transcript)
        db.execute(
            """INSERT OR REPLACE INTO transcripts (host, provider, transcript, parent,
                session, first, last, stray_closes)
            VALUES (?, ?, ?, ?, ?, ?, ?, coalesce((SELECT stray_closes FROM transcripts
                WHERE host = ? AND provider = ? AND transcript = ?), 0))""",
            (
                host,
                provider,
                transcript,
                parent,
                session_of(db, host, provider, top),
                first,
                last,
                host,
                provider,
                transcript,
            ),
        )
        db.execute(
            """UPDATE tool_calls SET end = r.end, result_bytes = r.result_bytes,
                compile_seconds = r.compile_seconds
            FROM tool_results AS r WHERE r.host = tool_calls.host
                AND r.provider = tool_calls.provider
                AND r.transcript = tool_calls.transcript AND r.call = tool_calls.call
                AND tool_calls.host = ? AND tool_calls.provider = ?
                AND tool_calls.transcript = ?""",
            (host, provider, transcript),
        )
    written = 0
    for top in sorted(tops):
        session = session_of(db, host, provider, top)
        found, stray = windows_of(db, host, provider, top, session)
        db.execute(
            "DELETE FROM windows WHERE host = ? AND provider = ? AND transcript = ?",
            (host, provider, top),
        )
        for window in found:
            db.execute(
                "INSERT OR REPLACE INTO windows (host, provider, session, transcript, "
                "start, end, bead, kind, label) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    host,
                    provider,
                    session,
                    top,
                    window.start,
                    window.end,
                    window.bead,
                    window.kind,
                    window.label,
                ),
            )
            written += 1
        db.execute(
            "UPDATE transcripts SET stray_closes = ? WHERE host = ? AND provider = ? "
            "AND transcript = ?",
            (stray, host, provider, top),
        )
        subagents = [
            str(r[0])
            for r in db.execute(
                "SELECT transcript FROM transcripts WHERE host = ? AND provider = ? "
                "AND parent = ?",
                (host, provider, top),
            )
        ]
        attribute(db, host, provider, top, found, subagents)
    return written
