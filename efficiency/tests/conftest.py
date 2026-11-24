"""Shared fixtures: a store in a temporary cache, and small synthetic sources. The repository
is public, so no test reads a real transcript, ticket or command line from a machine."""

from __future__ import annotations

import json
import sqlite3
from contextlib import closing
from datetime import datetime, timezone
from pathlib import Path

import pytest

from beryl_efficiency import changes, scope


@pytest.fixture
def utc_days(monkeypatch: pytest.MonkeyPatch) -> None:
    """Use UTC for the reports' calendar buckets on every host."""

    def as_utc(moment: datetime) -> datetime:
        if moment.tzinfo is None:
            return moment.replace(tzinfo=timezone.utc)
        return moment.astimezone(timezone.utc)

    monkeypatch.setattr(scope, "local_time", as_utc)


@pytest.fixture(autouse=True)
def cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Every test's `$BERYL_CACHE`, so none touches the machine's store."""
    path = tmp_path / "cache"
    monkeypatch.setenv("BERYL_CACHE", str(path))
    return path


@pytest.fixture(autouse=True)
def synthetic_history(monkeypatch: pytest.MonkeyPatch) -> None:
    """Keep synthetic reports independent of the checkout's staging history."""
    monkeypatch.setattr(changes, "file_versions", lambda checkout, ref="staging": [])


@pytest.fixture
def ignite_daemon(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A daemon with no local hub, with tables from Ignite's mirror and local schemas."""
    home = tmp_path / "ignite home"
    home.mkdir()
    monkeypatch.setenv("IGNITE_HOME", str(home))
    monkeypatch.delenv("IGNITE_HUB_DB", raising=False)
    created = "2026-11-15T10:00:00.000Z"
    landed = "2026-11-15T11:00:00.000Z"
    ticket = {
        "id": "t.1",
        "parent": "t",
        "title": "A check regression",
        "type": "bug",
        "status": "closed",
        "priority": 0,
        "labels": ["regression", "tier:opus"],
        "aliases": ["by-older.1"],
        "metadata": {"caused_by": "t.2", "culprit_check": "rust:engine::start"},
        "created_at": created,
        "claimed_at": created,
        "closed_at": landed,
        "landed_at": landed,
        "tree": "tree-07",
        "session": "cli-x",
        "landed": {"commits": ["abc"]},
    }
    epic = {
        "id": "t",
        "title": "An epic",
        "type": "epic",
        "status": "open",
        "priority": 1,
        "created_at": created,
    }
    with closing(sqlite3.connect(home / "mirror.db")) as db, db:
        db.execute(
            "CREATE TABLE versions (id INTEGER PRIMARY KEY, ticket TEXT, whole_json TEXT)"
        )
        db.executemany(
            "INSERT INTO versions (ticket, whole_json) VALUES (?, ?)",
            [
                (
                    "t.1",
                    json.dumps({**ticket, "title": "Earlier title", "status": "open"}),
                ),
                ("t", json.dumps(epic)),
                ("t.1", json.dumps(ticket)),
            ],
        )
    with closing(sqlite3.connect(home / "daemon.db")) as db, db:
        db.executescript("""
            CREATE TABLE passes (family TEXT, command TEXT, wall_s REAL, at TEXT);
            CREATE TABLE handles (id TEXT, verb TEXT, state TEXT, exit INTEGER,
                reply TEXT, started_at TEXT, finished_at TEXT);
            CREATE TABLE plan_usage (id INTEGER PRIMARY KEY, machine TEXT, provider TEXT,
                status TEXT, label TEXT, percent_used REAL, resets_at TEXT, at TEXT);
            """)
        db.executemany(
            "INSERT INTO passes VALUES ('rust', ?, ?, ?)",
            [("run", 8.0, created), ("list", 10.0, created), ("run", 7.0, landed)],
        )
        db.execute(
            "INSERT INTO handles VALUES ('T-1', 'tip-round', 'done', 0, ?, ?, ?)",
            (
                json.dumps({"json": {"tip": "abc", "outcome": {"state": "verified"}}}),
                created,
                landed,
            ),
        )
        db.execute(
            "INSERT INTO plan_usage VALUES (1, 'm-1', 'claude', 'ok', 'Weekly', 12.0, ?, ?)",
            (landed, created),
        )
    return home
