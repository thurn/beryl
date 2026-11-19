"""Ticket IDs as used by Enact and by the efficiency store."""

from __future__ import annotations

import re
import sqlite3

# Imported tickets keep their legacy root, which need not use the new alphabet.
PREFIX = "by-"
LEGACY = r"[a-z][a-z0-9]*-[a-z0-9]+(?:\.[0-9]+)*"
NATIVE = r"[a-z0-9]+(?:-[a-z0-9]+)*(?:\.[1-9][0-9]*)*"
DISPLAY = rf"T{NATIVE}"
# Ignite now stores and prints IDs in capitals with the `T` (`TQ44S.7`, `TCI-00031CED38`),
# or with an `E` for a root created as an epic (`EQ44S.7`) or an `M` for a meta-ticket. The efficiency store keeps the
# lowercase keys it had before, so `canonical_spelling` maps this spelling back to `Tq44s.7`
# (or `Eq44s.7`) before anything else reads it. Ignite never uses one root under two letters,
# so the key leaves the letter out (`by-q44s.7`).
CAPITAL = r"[TEM][A-Z0-9]+(?:-[A-Z0-9]+)*(?:\.[1-9][0-9]*)*"
ID = rf"(?:{LEGACY}|{CAPITAL}|{DISPLAY}|{NATIVE})"
IDENTIFIER: re.Pattern[str] = re.compile(ID)
# A bare name passed to worktree create is also allowed to be an ad hoc name.
WORKTREE_TICKET: re.Pattern[str] = re.compile(
    rf"(?:{LEGACY}|{CAPITAL}|{DISPLAY}|[abcdefghjkmnpqrstvwxyz][0-9abcdefghjkmnpqrstvwxyz]{{3}}(?:\.[1-9][0-9]*)*)"
)


def canonical_spelling(value: str) -> str:
    """The spelling from before Ignite stored IDs in capitals: `TQ44S.7` becomes `Tq44s.7`,
    and an epic's `EQ44S.7` becomes `Eq44s.7`."""
    if re.fullmatch(CAPITAL, value):
        return value[0] + value[1:].lower()
    return value


def prefixed(value: str) -> str:
    spelled = canonical_spelling(value)
    bare = spelled[1:] if spelled[:1] in ("T", "E", "M") else spelled
    return bare if bare.startswith(PREFIX) else PREFIX + bare


def stored(db: sqlite3.Connection, value: str) -> str:
    """Resolve a ticket spelling, retaining a moved ticket's original Tickets ID."""
    if not IDENTIFIER.fullmatch(value):
        return value
    value = canonical_spelling(value)
    name = prefixed(value)
    row = db.execute(
        "SELECT bead FROM ticket_aliases WHERE alias IN (?, ?) ORDER BY alias = ? DESC LIMIT 1",
        (value, name, value),
    ).fetchone()
    return str(row[0]) if row else value if re.fullmatch(LEGACY, value) else name
