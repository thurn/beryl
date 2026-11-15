"""`just efficiency bead <id>`: one bead's requests, tokens by class, the wall minutes inside its
windows, and the sessions that worked it. An epic's row includes the orchestration charged
to it."""

from __future__ import annotations

import json
import sqlite3
from datetime import datetime

from .render import Result, table

TOKENS: tuple[str, ...] = (
    "input_tokens",
    "cache_write_tokens",
    "cache_read_tokens",
    "output_tokens",
)


def minutes(start: str, end: str) -> float:
    return (
        datetime.fromisoformat(end) - datetime.fromisoformat(start)
    ).total_seconds() / 60


def hosts_of(db: sqlite3.Connection, bead: str) -> tuple[str | None, set[str]]:
    """The host the bead's metadata says it ran on, and the hosts the store has data from."""
    found = db.execute("SELECT enact FROM beads WHERE id = ?", (bead,)).fetchone()
    enact = json.loads(found[0]) if found and found[0] else {}
    ran = enact.get("enact_spawn_host") or enact.get("enact_worktree_host")
    have = {str(r[0]) for r in db.execute("SELECT DISTINCT host FROM transcripts")}
    return (str(ran) if ran else None), have


def report(db: sqlite3.Connection, bead: str) -> Result:
    title_row = db.execute("SELECT title FROM beads WHERE id = ?", (bead,)).fetchone()
    title = str(title_row[0]) if title_row and title_row[0] else None
    sums = ", ".join(f"coalesce(sum({c}), 0)" for c in TOKENS)
    found = db.execute(
        f"SELECT count(*), {sums} FROM requests WHERE bead = ?", (bead,)
    ).fetchone()
    requests = int(found[0])
    tokens = {name: int(v) for name, v in zip(TOKENS, found[1:])}
    sessions = [
        {
            "host": str(r[0]),
            "provider": str(r[1]),
            "session": str(r[2]),
            "transcript": str(r[3]),
            "title": r[4],
            "kind": str(r[5]),
            "start": str(r[6]),
            "end": str(r[7]),
            "minutes": round(minutes(str(r[6]), str(r[7])), 1),
        }
        for r in db.execute(
            """SELECT w.host, w.provider, w.session, w.transcript, s.title, w.kind, w.start,
                w.end
            FROM windows AS w LEFT JOIN sessions AS s ON s.host = w.host
                AND s.provider = w.provider AND s.transcript = w.transcript
            WHERE w.bead = ? ORDER BY w.start""",
            (bead,),
        )
    ]
    ran, have = hosts_of(db, bead)
    no_data = not sessions and ran is not None and ran not in have
    wall = round(sum(float(str(s["minutes"])) for s in sessions), 1)
    head = f"bead {bead}" + (f"  {title}" if title else "")
    if no_data:
        lines = [
            head,
            f"no data: it ran on {ran}, and the store has no sessions from there",
        ]
    elif not sessions and not requests:
        lines = [head, "no session in the store worked it"]
    else:
        lines = [
            head,
            f"requests {requests}  wall {wall:.1f} min in {len(sessions)} windows",
            "tokens   "
            + "  ".join(f"{k.removesuffix('_tokens')} {v}" for k, v in tokens.items()),
            "",
            *table(
                ["session", "title", "kind", "start", "minutes"],
                [
                    [s["session"], s["title"], s["kind"], s["start"], s["minutes"]]
                    for s in sessions
                ],
            ),
        ]
    return Result(
        "bead",
        {
            "bead": bead,
            "title": title,
            "no_data": no_data,
            "ran_on": ran,
            "requests": None if no_data else requests,
            "tokens": None if no_data else tokens,
            "wall_minutes": None if no_data else wall,
            "sessions": sessions,
        },
        lines,
        [f"efficiency sql \"select * from windows where bead = '{bead}'\""],
    )
