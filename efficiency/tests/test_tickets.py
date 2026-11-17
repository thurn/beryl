"""Ticket ingest over a store with pre-cutover history."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

from beryl_efficiency import (
    bead,
    beads,
    codex,
    cpu,
    facts,
    failures,
    ingest,
    rollups,
    store,
    tickets,
)
from beryl_efficiency.scope import Scope
from tests.synthetic import CHECKOUT, listing
from tests.synthetic import bead as bead_record
from tests.test_codex import HOST, OTHER, THREAD, Rollout
from tests.test_pushes import at


def ndjson(*rows: object) -> bytes:
    return "".join(json.dumps(row) + "\n" for row in rows).encode()


def test_backend_selection_uses_enacts_cutover_state(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    main: Path = tmp_path / "main"
    main.mkdir()
    checkout: Path = tmp_path / "old"
    config = main / ".enact.toml"
    config.write_text('# [tickets]\n# project = "by"\n')

    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=main, check=True, capture_output=True)

    git("init")
    git("add", ".enact.toml")
    git(
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "Dormant",
    )
    git("worktree", "add", "--detach", str(checkout))

    calls: list[str] = []
    backend: dict[str, str] = {"backend": "beads"}

    def read(cwd: Path, command: list[str]) -> bytes:
        assert cwd == (main if "bd" in command else checkout)
        calls.append(command[-1])
        return json.dumps(backend).encode() if command[-1] == "backend" else b""

    monkeypatch.setattr(beads, "run_command", read)
    assert tickets.sources(checkout)[2] is None
    assert calls == []
    config.write_text('[tickets]\nproject = "by"\nurl = "http://localhost:8888"\n')
    records, verdicts, events = tickets.sources(checkout)
    assert calls == []
    assert records.read() == b""
    assert events is not None and events.page(0)[1] == 0
    assert calls == ["backend", "--json"]
    backend.update(backend="server", tk="/fixture/tk")
    records, verdicts, events = tickets.sources(checkout)
    assert events is not None
    assert records.read() == verdicts.read() == b""
    assert calls == ["backend", "--json", "backend", "export"]


@pytest.mark.parametrize("prefix", ["", "T"])
def test_one_store_attributes_old_and_new_sessions_ci_and_leases(
    tmp_path: Path,
    prefix: str,
) -> None:
    old = "by-previous.7"
    metadata = {"enact_worktree": "/fixtures/tree", "enact_ci_range": "green..bad"}
    legacy = listing(bead_record(old, started_at=at(10, 0), metadata=metadata))
    legacy_log = (
        Rollout(tmp_path, THREAD)
        .turn(at(10, 0))
        .shell(at(10, 1), "old-start", f"enact start {old}", at(10, 2), "")
        .request(at(10, 3), 100, 0, 5)
    )
    legacy_log.write()
    db = store.open_store(tmp_path / "store.sqlite")
    logs = codex.Rollouts([tmp_path / "sessions"], CHECKOUT, HOST)
    ingest.ingest(db, [beads.Beads(lambda: legacy), logs], HOST)

    # The importer allocated a different native ID after this child was moved.
    export: bytes = ndjson(
        {"id": f"{prefix}k7mq", "title": "Native epic", "type": "epic", "labels": []},
        {
            "id": f"{prefix}k7mq.2",
            "aliases": [old],
            "parent": f"{prefix}k7mq",
            "title": "Migrated Steward",
            "type": "task",
            "created_at": at(10, 0),
            "metadata": {**metadata, "_enact_beads": {"started_at": at(10, 0)}},
        },
        {
            "id": f"{prefix}k7mq.3",
            "parent": f"{prefix}k7mq",
            "title": "Native ticket",
            "type": "task",
            "metadata": {"enact_worktree": "/fixtures/native"},
        },
        {
            "id": f"{prefix}c5ck",
            "title": "repo:check",
            "labels": ["ci-check"],
            "comments": [
                {
                    "n": 1,
                    "legacy_id": "old-comment",
                    "text": json.dumps(
                        {"time": at(10, 2), "verdict": "flaky", "steward": "k7mq.2"}
                    ),
                },
                {
                    "n": 2,
                    "text": json.dumps({"corrects": "old-comment", "verdict": "catch"}),
                },
                {"n": 3, "text": json.dumps({"time": at(11, 6), "verdict": "catch"})},
                {
                    "n": 4,
                    "text": json.dumps({"corrects": "ticket:3", "verdict": "flaky"}),
                },
            ],
        },
    )
    events: dict[int, dict[str, object]] = {
        2: {"kind": "machine", "data": {}},
        4: {
            "kind": "ci",
            "at": at(10, 2),
            "data": {
                "tip": "bad",
                "passed": False,
                "ticket": f"{prefix}k7mq.2",
                "host": HOST,
            },
        },
        5: {
            "kind": "ci",
            "at": at(10, 3),
            "data": {"tip": "good", "passed": True, "host": HOST},
        },
    }
    cursors: list[int] = []

    def read(*args: str) -> bytes:
        if args == ("export",):
            return export
        assert args[:2] == ("events", "--since")
        after = int(args[2])
        cursors.append(after)
        return json.dumps(
            [{"seq": seq, **events[seq]} for seq in sorted(events) if seq > after][:2]
        ).encode()

    legacy_log.shell(
        at(10, 4), "old-close", f"enact close {prefix}k7mq.2", at(10, 5), ""
    ).write()
    (
        Rollout(tmp_path, OTHER)
        .turn(at(11, 0))
        .request(at(11, 1), 200, 0, 10)
        .shell(at(11, 3), "wait", "tk --json wait lease staging", at(11, 4), "")
        .shell(at(11, 5), "push", "enact push --bead by-k7mq.3", at(11, 8), "")
        .request(at(11, 9), 200, 0, 10)
        .request(at(11, 11), 200, 0, 10)
        .write()
    )
    records, verdicts, native = tickets.native_sources(read)
    lease_log = tmp_path / "leases.jsonl"
    lease_log.write_bytes(
        ndjson(
            {
                "time": at(11, 6),
                "class": "agent",
                "worktree": "native",
                "command": "check",
                "cores": 1,
                "wanted": 1,
                "waited_s": 12,
                "held_s": 10,
            }
        )
    )
    sources: list[ingest.Source] = [
        records,
        native,
        logs,
        cpu.Leases(lease_log, HOST),
        verdicts,
    ]
    ingest.ingest(db, sources, HOST)
    assert cursors == [0, 4, 5]
    assert db.execute(
        "SELECT bead FROM windows WHERE transcript = ?", (OTHER,)
    ).fetchone() == (None,)

    # The native start and close arrive after the transcript was read.
    for seq, action, minute in [(7, "start", 2), (8, "close", 10)]:
        events[seq] = {
            "kind": "session",
            "at": at(11, minute),
            "data": {"ticket": f"{prefix}k7mq.3", "session": OTHER, "action": action},
        }
    ingest.ingest(db, sources, HOST)
    assert cursors == [0, 4, 5, 5, 8]
    assert db.execute(
        "SELECT transcript, bead FROM windows WHERE kind='bead' ORDER BY start"
    ).fetchall() == [(THREAD, old), (OTHER, "by-k7mq.3")]
    assert db.execute(
        "SELECT started_at FROM beads WHERE id = ?", (old,)
    ).fetchone() == (
        at(10, 0),
    )
    assert db.execute(
        "SELECT verdict, steward FROM verdicts ORDER BY time"
    ).fetchall() == [
        ("catch", old),
        ("flaky", None),
    ]
    assert db.execute("SELECT call FROM leases").fetchone() == ("push",)
    assert db.execute(
        "SELECT bead, kind FROM tool_calls WHERE call = 'wait'"
    ).fetchone() == ("by-k7mq.3", "enact lease wait")
    assert facts.load(db).beads["by-k7mq.3"].started == at(11, 2)
    assert bead.report(db, old).json() == bead.report(db, "k7mq.2").json()
    assert bead.report(db, old).json() == bead.report(db, "Tk7mq.2").json()
    assert (
        rollups.epic(facts.load(db), Scope(), "k7mq").json()
        == rollups.epic(facts.load(db), Scope(), "by-k7mq").json()
    )
    result = failures.all_failures(db, facts.load(db))[0]
    assert (result.steward, result.failed_at, result.host, result.behind_minutes) == (
        old,
        at(10, 2),
        HOST,
        1,
    )
    # Cursor commits are independent of snapshots and survive a temporarily empty feed.
    ingest.ingest(db, sources, HOST)
    assert cursors[-1] == 8
    assert db.execute("SELECT count(*) FROM ticket_ci_events").fetchone() == (2,)
