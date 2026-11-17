"""`just efficiency session <id>`: one session's requests and tool calls in order, with its
subagents' rows marked. The id is a desktop session (`local_<uuid>`), a transcript, or a
prefix of either that names one session."""

from __future__ import annotations

import sqlite3
from dataclasses import asdict, dataclass

from .facts import Facts, money
from .render import Result, describe, mark, table
from .scope import Scope

# How many rows the text shows; `--json` has them all.
SHOWN = 200


@dataclass
class Row:
    time: str
    what: str
    subagent: bool
    detail: str | None
    context_tokens: int | None
    output_tokens: int | None
    dollars: float | None
    seconds: float | None
    bead: str | None


@dataclass
class Session:
    session: str
    title: str | None
    transcripts: list[str]
    requests: int
    tool_calls: int
    dollars: float
    lower_bound: bool
    beads: list[str]
    hidden: int
    rows: list[Row]


def find(db: sqlite3.Connection, given: str) -> tuple[str, list[str]]:
    """The session `given` names and its top-level transcripts."""
    found: dict[str, list[str]] = {}
    for session, transcript in db.execute(
        """SELECT session, transcript FROM transcripts WHERE parent IS NULL
            AND (session LIKE ? || '%' OR transcript LIKE ? || '%')
        ORDER BY first""",
        (given, given),
    ):
        found.setdefault(str(session), []).append(str(transcript))
    if given in found:
        return given, found[given]
    if len(found) != 1:
        names = ", ".join(sorted(found)[:5]) or "none"
        raise KeyError(f"{given} names {len(found)} sessions: {names}")
    return next(iter(found.items()))


def report(
    db: sqlite3.Connection,
    facts: Facts,
    scope: Scope,
    given: str,
    bead: str | None = None,
) -> Result:
    session, tops = find(db, given)
    marks = ",".join("?" for _ in tops)
    transcripts = [
        (str(r[0]), r[1] is not None)
        for r in db.execute(
            f"""SELECT transcript, parent FROM transcripts
            WHERE transcript IN ({marks}) OR parent IN ({marks})""",
            tops + tops,
        )
    ]
    subagent = dict(transcripts)
    names = [t for t, _ in transcripts]
    held = ",".join("?" for _ in names)
    beads = facts.cohort(scope)
    rows: list[Row] = []
    for r in db.execute(
        f"""SELECT time, 'request', transcript, coalesce(model, '-'), stop_reason,
                context_tokens, output_tokens, dollars, NULL, bead, host, provider
            FROM requests WHERE transcript IN ({held})
        UNION ALL
        SELECT start, 'tool', transcript, coalesce(kind, tool), substr(command, 1, 80),
                NULL, NULL, NULL, (julianday(end) - julianday(start)) * 86400, bead,
                host, provider
            FROM tool_calls WHERE transcript IN ({held})
        ORDER BY 1""",
        names + names,
    ):
        if not (
            scope.range.holds(str(r[0]))
            and (scope.host is None or r[10] == scope.host)
            and (scope.provider is None or r[11] == scope.provider)
            and (beads is None or r[9] in beads)
            and (bead is None or r[9] == bead)
        ):
            continue
        detail = str(r[4]).replace("\n", " ") if r[4] is not None else None
        rows.append(
            Row(
                str(r[0]),
                f"{r[1]} {r[3]}",
                subagent.get(str(r[2]), False),
                detail,
                None if r[5] is None else int(r[5]),
                None if r[6] is None else int(r[6]),
                None if r[7] is None else money(float(r[7])),
                None if r[8] is None else round(float(r[8]), 1),
                r[9],
            )
        )
    requests = [r for r in rows if r.what.startswith("request")]
    title = db.execute(
        "SELECT title FROM sessions WHERE session = ? AND title IS NOT NULL LIMIT 1",
        (session,),
    ).fetchone()
    result = Session(
        session,
        str(title[0]) if title else None,
        tops,
        len(requests),
        len(rows) - len(requests),
        money(sum(r.dollars or 0.0 for r in requests)),
        any(r.dollars is None for r in requests),
        sorted({r.bead for r in rows if r.bead}),
        max(len(rows) - SHOWN, 0),
        rows,
    )
    return Result(
        "session",
        asdict(result),
        lines(result),
        [f"efficiency ticket {b}" for b in result.beads][:3],
        describe("requests", "dollars", "lower_bound", "rows"),
    )


def lines(s: Session) -> list[str]:
    shown = s.rows[: len(s.rows) - s.hidden] if s.hidden else s.rows
    return [
        f"session {s.session}" + (f"  {s.title}" if s.title else ""),
        f"requests {s.requests}   tool calls {s.tool_calls}   "
        + f"dollars {s.dollars:.2f}{mark(s.lower_bound)}   tickets "
        + (", ".join(s.beads) or "none"),
        "",
        *table(
            [
                "time",
                "what",
                "context",
                "output",
                "dollars",
                "seconds",
                "bead",
                "detail",
            ],
            [
                [
                    r.time,
                    ("  " if r.subagent else "") + r.what,
                    r.context_tokens,
                    r.output_tokens,
                    r.dollars,
                    r.seconds,
                    r.bead,
                    r.detail,
                ]
                for r in shown
            ],
        ),
        *(
            [f"{s.hidden} later rows not shown; --json prints them all"]
            if s.hidden
            else []
        ),
    ]
