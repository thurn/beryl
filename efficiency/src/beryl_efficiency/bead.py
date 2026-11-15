"""`just efficiency bead <id>`: one bead's requests, tokens by class, the wall minutes inside its
windows, and the sessions that worked it. An epic's row includes the orchestration charged
to it."""

from __future__ import annotations

import json
import sqlite3
from datetime import datetime

from .render import Result, table
from .timing import AGENT_CATEGORIES

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


def time_split(db: sqlite3.Connection, bead: str) -> list[dict[str, object]]:
    """The bead's windows' minutes by category, kind and part, largest first."""
    return [
        {
            "category": str(r[0]),
            "kind": str(r[1]),
            "part": str(r[2]),
            "minutes": round(float(r[3]) / 60, 2),
        }
        for r in db.execute(
            """SELECT t.category, t.kind, t.part, sum(t.seconds) AS s FROM window_time AS t
            JOIN windows AS w ON w.host = t.host AND w.provider = t.provider
                AND w.transcript = t.transcript AND w.start = t.start
            WHERE w.bead = ? GROUP BY t.category, t.kind, t.part ORDER BY s DESC""",
            (bead,),
        )
    ]


def time_lines(split: list[dict[str, object]]) -> list[str]:
    """Agent minutes, then each category and kind (parts indented under it), then the
    minutes beside agent minutes."""
    totals: dict[tuple[str, str], float] = {}
    parts: dict[tuple[str, str], list[tuple[str, float]]] = {}
    for row in split:
        key = (str(row["category"]), str(row["kind"]))
        value = float(str(row["minutes"]))
        totals[key] = totals.get(key, 0.0) + value
        if row["part"]:
            parts.setdefault(key, []).append((str(row["part"]), value))
    agent = sum(v for (c, _), v in totals.items() if c in AGENT_CATEGORIES)
    lines = [f"time      {agent:.1f} agent min"]
    for (category, kind), value in sorted(totals.items(), key=lambda kv: -kv[1]):
        if category not in AGENT_CATEGORIES or value < 0.05:
            continue
        name = f"{category}: {kind}" if kind else category
        lines.append(f"  {name:<34}{value:7.1f}")
        for part, minutes in parts.get((category, kind), []):
            if minutes >= 0.05:
                lines.append(f"      {part:<30}{minutes:7.1f}")
    beside = {
        c: sum(v for (k, _), v in totals.items() if k == c) for c in ("person", "idle")
    }
    lines.append(
        f"beside agent minutes: waiting on a person {beside['person']:.1f}, "
        f"idle {beside['idle']:.1f}"
    )
    return lines


def dollar_split(db: sqlite3.Connection, bead: str) -> dict[str, dict[str, float]]:
    """The bead's windows' dollars by split (`class`, `source`, `purpose`) and name, and the
    models with no price under `unpriced` with their tokens."""
    found: dict[str, dict[str, float]] = {}
    for split, name, tokens, dollars in db.execute(
        """SELECT d.split, d.name, sum(d.tokens), sum(d.dollars) FROM window_dollars AS d
        JOIN windows AS w ON w.host = d.host AND w.provider = d.provider
            AND w.transcript = d.transcript AND w.start = d.start
        WHERE w.bead = ? GROUP BY d.split, d.name""",
        (bead,),
    ):
        value = float(tokens) if split == "unpriced" else float(dollars)
        found.setdefault(str(split), {})[str(name)] = round(value, 4)
    return found


def dollar_lines(split: dict[str, dict[str, float]]) -> list[str]:
    """The total, marked as a lower bound when a model had no price, then each split."""
    total = sum(split.get("class", {}).values())
    unpriced = split.get("unpriced", {})
    mark = " or more (no price for " + ", ".join(unpriced) + ")" if unpriced else ""
    lines = [f"dollars   {total:.2f}{mark}"]
    for name in ("class", "source", "purpose"):
        parts = sorted(split.get(name, {}).items(), key=lambda kv: -kv[1])
        shown = " | ".join(f"{k} {v:.2f}" for k, v in parts if v >= 0.005)
        lines.append(f"  by {name:<8}{shown or '-'}")
    return lines


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
    split = time_split(db, bead)
    money = dollar_split(db, bead)
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
            *time_lines(split),
            *dollar_lines(money),
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
            "time": None if no_data else split,
            "dollars": None if no_data else money,
            "sessions": sessions,
        },
        lines,
        [f"efficiency sql \"select * from windows where bead = '{bead}'\""],
    )
