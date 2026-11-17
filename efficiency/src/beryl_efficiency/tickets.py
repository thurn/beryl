"""The native Enact ticket snapshot and its session and CI event feed."""

from __future__ import annotations

import json
import sqlite3
import sys
from collections import defaultdict
from collections.abc import Callable
from functools import cache
from pathlib import Path

from . import beads, cilog, ticket_ids, timing, tokens, windows
from .ingest import CursorSource, Row, SnapshotSource, SourceError

Read = Callable[..., bytes]


def sources(checkout: Path) -> tuple[SnapshotSource, SnapshotSource, CursorSource]:
    """Discover Enact's native client once per ingest."""

    @cache
    def binary() -> str:
        helper = Path.home() / "enact" / "scripts" / "enact.py"
        try:
            active = json.loads(
                beads.run_command(checkout, [sys.executable, str(helper), "backend"])
            )
            if active["backend"] == "server" and isinstance(active["tk"], str):
                return str(active["tk"])
            raise ValueError("unexpected Enact backend reply")
        except (ValueError, KeyError, TypeError) as error:
            raise SourceError(f"Enact backend: {error}") from error

    def tk(*args: str) -> bytes:
        return beads.run_command(
            checkout, [binary(), "--json", "--retry-for", "2", *args]
        )

    return native_sources(tk)


def native_sources(read: Read) -> tuple[SnapshotSource, SnapshotSource, CursorSource]:
    @cache
    def export() -> bytes:
        # Give both snapshots the same view and read comments without one show per ticket.
        return read("export")

    return Tickets(export), TicketVerdicts(export), Events(read)


def parse_export(data: bytes) -> list[dict[str, object]]:
    try:
        records = [json.loads(line) for line in data.splitlines() if line.strip()]
        if any(
            not isinstance(row, dict) or not isinstance(row.get("id"), str)
            for row in records
        ):
            raise TypeError("ticket row has no id")
    except (ValueError, TypeError) as error:
        raise SourceError(f"invalid tk export: {error}") from error
    return records


def ids(records: list[dict[str, object]]) -> dict[str, str]:
    """Keep the original ID for moved children and other imported IDs that changed."""
    result: dict[str, str] = {}
    for record in records:
        ticket = str(record["id"]).removeprefix("T")
        aliases = record.get("aliases")
        legacy = (
            [
                a
                for a in aliases
                if isinstance(a, str) and a.startswith(ticket_ids.PREFIX)
            ]
            if isinstance(aliases, list)
            else []
        )
        # tk import supplies the original Tickets ID. Prefer the ordinary prefixed spelling
        # if it is itself an alias, or if this ticket has no imported Tickets ID.
        prefixed = ticket_ids.prefixed(ticket)
        bead = prefixed if prefixed in legacy else next(iter(legacy), prefixed)
        for alias in (ticket, f"T{ticket}", prefixed, *legacy):
            result[alias] = bead
    return result


def reference(value: object, aliases: dict[str, str]) -> object:
    return aliases.get(value, value) if isinstance(value, str) else value


class Tickets(beads.Beads):
    def parse(self, data: bytes) -> tuple[list[Row], int]:
        records = parse_export(data)
        aliases = ids(records)
        rows: list[Row] = []
        for alias, bead in aliases.items():
            rows.append(("ticket_aliases", {"alias": alias, "bead": bead}))
        for record in records:
            metadata = record.get("metadata")
            imported = (
                metadata.get("_enact_beads") if isinstance(metadata, dict) else None
            )
            try:
                rows += beads.rows_of(
                    {
                        **record,
                        "id": aliases[str(record["id"])],
                        "parent": reference(record.get("parent"), aliases),
                        "issue_type": record.get("type"),
                        "started_at": (
                            imported.get("started_at")
                            if isinstance(imported, dict)
                            else None
                        ),
                    }
                )
            except (TypeError, KeyError) as error:
                raise SourceError(
                    f"invalid tk export row {record['id']}: {error}"
                ) from error
        return rows, 0


class TicketVerdicts(cilog.Verdicts):
    def parse(self, data: bytes) -> tuple[list[Row], int]:
        records = parse_export(data)
        aliases = ids(records)
        rows: list[Row] = []
        for record in records:
            labels = record.get("labels")
            if not isinstance(labels, list) or cilog.CHECK_LABEL not in labels:
                continue
            comments = record.get("comments")
            if not isinstance(comments, list):
                raise SourceError(f"tk export has no comments for {record['id']}")
            ticket = ticket_ids.prefixed(str(record["id"]))
            with_ids: list[dict[str, object]] = []
            comment_aliases: dict[str, str] = {}
            for comment in comments:
                if not isinstance(comment, dict) or "n" not in comment:
                    continue
                number = comment["n"]
                key = str(comment.get("legacy_id") or f"{ticket}:{number}")
                with_ids.append({**comment, "id": key})
                # Native verdicts.py writes corrections using the comment's sequence
                # within this ticket. Keep the full ID as SQLite's row key.
                comment_aliases[f"ticket:{number}"] = key
            rows += cilog.verdict_rows(
                {**record, "comments": with_ids}, comment_aliases
            )
        for _, row in rows:
            row["steward"] = reference(row["steward"], aliases)
        return rows, 0


def event_row(event: dict[str, object], seq: int) -> Row | None:
    data = event.get("data")
    if not isinstance(data, dict):
        return None
    ticket = data.get("ticket")
    kind = event.get("kind")
    if kind == "session" and data.get("action") in ("start", "close"):
        session = beads.text(data, "session") or beads.text(event, "actor")
        if not isinstance(ticket, str) or not session:
            raise TypeError("session event needs a ticket and a session")
        return (
            "ticket_session_events",
            {
                "seq": seq,
                "time": cilog.iso(cilog.utc(event["at"])),
                "session": session,
                "verb": "open" if data["action"] == "start" else "close",
                "ticket": ticket,
            },
        )
    if (
        kind == "ci"
        and isinstance(data.get("tip"), str)
        and isinstance(data.get("passed"), bool)
    ):
        return (
            "ticket_ci_events",
            {
                "seq": seq,
                "time": cilog.iso(cilog.utc(event["at"])),
                "host": cilog.host_name(data.get("host") or event.get("machine")),
                "tip": data["tip"],
                "passed": data["passed"],
                "ticket": beads.text(data, "ticket"),
            },
        )
    return None


class Events(CursorSource):
    name = "ticket_events"

    def __init__(self, read: Read) -> None:
        self.read = read

    def page(self, after: int) -> tuple[bytes, int, list[Row], int]:
        data = self.read("events", "--since", str(after))
        rows: list[Row] = []
        skipped = 0
        try:
            events = json.loads(data)
            if not isinstance(events, list):
                raise TypeError("expected a list")
            for event in events:
                seq = event["seq"]
                if not isinstance(seq, int) or isinstance(seq, bool) or seq <= after:
                    raise ValueError("event sequence did not advance")
                after = seq
                try:
                    if row := event_row(event, seq):
                        rows.append(row)
                except (ValueError, KeyError, TypeError):
                    skipped += 1
        except (ValueError, KeyError, TypeError) as error:
            raise SourceError(f"invalid tk events page: {error}") from error
        return data, after, rows, skipped

    def save(self, db: sqlite3.Connection, rows: list[Row]) -> int:
        count = super().save(db, rows)
        changed: dict[tuple[str, str], set[str]] = defaultdict(set)
        for table, row in rows:
            if table != "ticket_session_events":
                continue
            for host, provider, transcript in db.execute(
                """SELECT host, provider, transcript FROM transcripts
                WHERE parent IS NULL AND (session = ?1 OR transcript = ?1)
                    AND first <= ?2 AND last >= ?2""",
                (row["session"], row["time"]),
            ):
                changed[(str(host), str(provider))].add(str(transcript))
        for (host, provider), tops in changed.items():
            ordered = sorted(tops)
            count += windows.derive(db, host, provider, [(t, None) for t in ordered])
            count += timing.refresh(db, host, provider, ordered)
            count += tokens.refresh(db, host, provider, ordered)
        return count
