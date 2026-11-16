"""Each bead's changelog: its audit events from `bd history <id> --events --json`, cached per
bead and refetched only when the bead's `updated_at` moves.

The command takes about half a second per bead, so no page waits on it while it renders: a
page shows a cached changelog that is still current, and otherwise loads it afterwards as a
fragment. Fetching every bead once takes tens of minutes; after that only changed beads are
read again. Some beads have no events (`bd` returns `null`), and their changelog is the
lifecycle that the bead's own fields record: created, started and closed."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from . import paths
from .beads import Bead

# Fields an `updated` event can carry that a reader does not need, because Enact rewrites them
# on every claim and release.
QUIET_FIELDS: frozenset[str] = frozenset({"metadata", "updated_at"})


@dataclass(frozen=True)
class Event:
    """One audit event: when, who, what kind, and what it changed. `fields` holds the fields
    an `updated` or `claimed` event set, and `text` the message of other kinds."""

    at: str
    actor: str
    kind: str
    text: str = ""
    fields: tuple[tuple[str, str], ...] = ()

    @property
    def quiet(self) -> bool:
        """Whether the event changed only bookkeeping that a reader can skip."""
        return self.kind == "updated" and not self.fields


@dataclass(frozen=True)
class Changelog:
    """A bead's events, newest first, as of the bead's `updated_at`. `recorded` is False when
    `bd` had no events for the bead."""

    updated_at: str
    events: tuple[Event, ...]
    recorded: bool = True


def low_priority() -> list[str]:
    """The prefix that runs `bd` below normal priority, so filling the cache yields to the
    machine's real work. It is `nice` and not macOS's utility class, which gets almost no CPU
    at this machine's usual load (tools/AGENTS.md): at that class one bead took about 20
    seconds instead of half a second."""
    return [] if sys.platform == "win32" else ["nice", "-n", "10"]


def value_text(value: object) -> str:
    if isinstance(value, str):
        return value
    return json.dumps(value, sort_keys=True)


def parse_event(raw: Mapping[str, object]) -> Event:
    kind = str(raw.get("event_type") or "event")
    fields: list[tuple[str, str]] = []
    message = str(raw.get("comment") or "")
    new = raw.get("new_value")
    if isinstance(new, str) and new.startswith("{"):
        try:
            changed = json.loads(new)
        except json.JSONDecodeError:
            changed = None
        if isinstance(changed, dict):
            fields = sorted(
                (str(k), value_text(v))
                for k, v in changed.items()
                if k not in QUIET_FIELDS
            )
    elif isinstance(new, str) and new and not message:
        message = new
    return Event(
        at=str(raw.get("created_at") or ""),
        actor=str(raw.get("actor") or ""),
        kind=kind,
        text=message,
        fields=tuple(fields),
    )


def parse(output: str, updated_at: str) -> Changelog:
    data = json.loads(output) if output.strip() else None
    if not isinstance(data, list):
        return Changelog(updated_at, (), recorded=False)
    events = [parse_event(e) for e in data if isinstance(e, dict)]
    events.sort(key=lambda e: e.at, reverse=True)
    return Changelog(updated_at, tuple(events))


def fetch(main: Path, bead: Bead) -> Changelog:
    """Runs `bd` for one bead's events."""
    done = subprocess.run(
        [*low_priority(), "bd", "history", bead.id, "--events", "--json"],
        cwd=main,
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode:
        raise RuntimeError(done.stderr.strip() or f"bd history {bead.id} failed")
    return parse(done.stdout, bead.updated_at)


def lifecycle(bead: Bead) -> Changelog:
    """The changelog of a bead with no recorded events, from its own timestamps."""
    events = [Event(bead.created_at, bead.created_by, "created")]
    if bead.started_at:
        events.append(Event(bead.started_at, "", "started"))
    if bead.closed_at:
        events.append(Event(bead.closed_at, "", "closed", bead.close_reason))
    events.sort(key=lambda e: e.at, reverse=True)
    return Changelog(bead.updated_at, tuple(events), recorded=False)


def cache_file(bead_id: str) -> Path:
    return paths.changelogs() / f"{bead_id}.json"


def to_json(log: Changelog) -> str:
    return json.dumps(
        {
            "updated_at": log.updated_at,
            "recorded": log.recorded,
            "events": [
                {
                    "at": e.at,
                    "actor": e.actor,
                    "kind": e.kind,
                    "text": e.text,
                    "fields": [list(f) for f in e.fields],
                }
                for e in log.events
            ],
        },
        separators=(",", ":"),
    )


def from_json(data: str) -> Changelog | None:
    try:
        raw = json.loads(data)
    except json.JSONDecodeError:
        return None
    if not isinstance(raw, dict) or not isinstance(raw.get("events"), list):
        return None
    events: list[Event] = []
    for e in raw["events"]:
        if not isinstance(e, dict):
            continue
        fields = e.get("fields")
        events.append(
            Event(
                str(e.get("at") or ""),
                str(e.get("actor") or ""),
                str(e.get("kind") or ""),
                str(e.get("text") or ""),
                tuple(
                    (str(f[0]), str(f[1]))
                    for f in (fields if isinstance(fields, list) else [])
                    if isinstance(f, list) and len(f) == 2
                ),
            )
        )
    return Changelog(
        str(raw.get("updated_at") or ""),
        tuple(events),
        raw.get("recorded") is not False,
    )


def cached(bead: Bead) -> Changelog | None:
    """The cached changelog, if it is still current: fetched since the bead last changed."""
    path = cache_file(bead.id)
    try:
        log = from_json(path.read_text(encoding="utf-8"))
    except OSError:
        return None
    return log if log is not None and log.updated_at == bead.updated_at else None


def store(bead_id: str, log: Changelog) -> None:
    """Writes the cache file in one step, so a reader never sees half of it."""
    target = cache_file(bead_id)
    target.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(dir=target.parent, suffix=".tmp")
    with os.fdopen(handle, "w", encoding="utf-8") as out:
        out.write(to_json(log))
    os.replace(temporary, target)


def current(main: Path, bead: Bead) -> Changelog:
    """The bead's changelog, from the cache if it is current, else from `bd`, cached."""
    found = cached(bead)
    if found is not None:
        return found
    log = fetch(main, bead)
    store(bead.id, log)
    return log


def shown(bead: Bead, log: Changelog | None) -> Changelog | None:
    """What a page shows for the bead: its events, or its lifecycle when `bd` had none."""
    if log is not None and not log.recorded:
        return lifecycle(bead)
    return log
