"""Ticket IDs as used by Enact and by the efficiency store."""

from __future__ import annotations

import re
import sqlite3

# Imported tickets keep their legacy root, which need not use the new alphabet.
PREFIX = "by-"
LEGACY = r"[a-z][a-z0-9]*-[a-z0-9]+(?:\.[0-9]+)*"
NATIVE = r"[a-z0-9]+(?:-[a-z0-9]+)*(?:\.[1-9][0-9]*)*"
DISPLAY = rf"T{NATIVE}"
ID = rf"(?:{LEGACY}|{DISPLAY}|{NATIVE})"
IDENTIFIER: re.Pattern[str] = re.compile(ID)
# A bare name passed to worktree create is also allowed to be an ad hoc name.
WORKTREE_TICKET: re.Pattern[str] = re.compile(
    rf"(?:{LEGACY}|{DISPLAY}|[abcdefghjkmnpqrstvwxyz][0-9abcdefghjkmnpqrstvwxyz]{{3}}(?:\.[1-9][0-9]*)*)"
)


def prefixed(value: str) -> str:
    bare = value.removeprefix("T")
    return bare if bare.startswith(PREFIX) else PREFIX + bare


def stored(db: sqlite3.Connection, value: str) -> str:
    """Resolve a ticket spelling, retaining a moved ticket's original Beads ID."""
    if not IDENTIFIER.fullmatch(value):
        return value
    name = prefixed(value)
    row = db.execute(
        "SELECT bead FROM ticket_aliases WHERE alias IN (?, ?) ORDER BY alias = ? DESC LIMIT 1",
        (value, name, value),
    ).fetchone()
    return str(row[0]) if row else value if re.fullmatch(LEGACY, value) else name
