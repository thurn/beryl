"""Ticket windows as segments for the hub's ticket pages."""

from __future__ import annotations

import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import claude, ingest, spend, store
from tests.legacy_sources import Beads
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record

HOST = "jarvis"
OPUS = "claude-opus-5-5"
ENACT = "python3 ~/enact/scripts/enact.py"
FINISHED = "    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.00s"


def at(seconds: int) -> str:
    return f"2026-11-15T10:{seconds // 60:02d}:{seconds % 60:02d}.000Z"


def ingested(root: Path) -> sqlite3.Connection:
    db = store.open_store(root / "store.sqlite")
    sources: list[ingest.Source] = [
        Beads(lambda: listing(bead_record("by-a"))),
        claude.Sessions(root / "sessions", CHECKOUT, HOST),
        claude.Transcripts(root / "projects", CHECKOUT, HOST),
    ]
    ingest.ingest(db, sources, HOST)
    return db


def one_ticket(root: Path) -> None:
    """A session starts ticket `by-a`, runs cargo test for 30 seconds (6 of them
    compiling), replies, waits 55 seconds for a person and replies again."""
    (
        Transcript(root / "projects" / PROJECT / "t1.jsonl")
        .bash(at(0), "m1", "s", f"{ENACT} start by-a", model=OPUS)
        .result(at(1), "s")
        .request(
            at(10),
            "m2",
            usage=(10, 90, 1000, 50),
            model=OPUS,
            tools=(("c", "Bash", {"command": "cd rust && cargo test"}),),
        )
        .result(at(40), "c", stderr=FINISHED)
        .request(at(45), "m3", usage=(10, 90, 2000, 400), model=OPUS)
        .prompt(at(100))
        .request(at(110), "m4", usage=(10, 90, 2000, 100), model=OPUS)
        .save()
    )


def test_a_window_becomes_time_and_dollar_segments_in_order(tmp_path: Path) -> None:
    one_ticket(tmp_path)
    db = ingested(tmp_path)
    [window] = spend.windows(db, HOST, None)
    # Sent without its letter: the hub stores it under `TA` or an epic's `EA`.
    assert window["ticket"] == "A"
    time = window["time"]
    assert isinstance(time, list)
    keys = [(s["c"], s["k"], s.get("p", "")) for s in time]
    compile_at = keys.index(("tool", "rust tests", "compile"))
    assert keys[compile_at + 1] == ("tool", "rust tests", "run")
    assert time[compile_at]["s"] == pytest.approx(6.0)
    assert time[compile_at]["x"] == "cd rust && cargo test"
    assert ("person", "", "") in keys
    assert {s["g"] for s in time} == {"ignite", "model", "checks", "person"}
    # Every second of the window is in exactly one segment.
    length = spend.timing.seconds(str(window["end"])) - spend.timing.seconds(
        str(window["start"])
    )
    assert sum(float(s["s"]) for s in time) == pytest.approx(length, abs=0.05)
    dollars = window["dollars"]
    assert isinstance(dollars, list)
    kinds = [s["k"] for s in dollars]
    assert kinds[kinds.index("rust tests") + 1] == spend.REPLY
    assert all(float(s["d"]) > 0 for s in dollars)
    assert sum(int(s["r"]) for s in dollars) == 4


def test_windows_that_ended_before_since_are_left_out(tmp_path: Path) -> None:
    one_ticket(tmp_path)
    db = ingested(tmp_path)
    assert spend.windows(db, HOST, "2026-11-16T00:00:00.000Z") == []


def test_coalescing_keeps_the_total_and_every_break(tmp_path: Path) -> None:
    segments: list[spend.Segment] = []
    for index in range(600):
        if index % 100 == 50:
            segments.append(spend.Segment("person", "", "", float(index), 1000.0))
        else:
            kind = "git" if index % 2 else "read and search"
            segments.append(
                spend.Segment("tool", kind, "", float(index), 1.0 + index % 3)
            )
    found = spend.coalesced(segments, limit=40)
    assert len(found) <= 40 + 6 + 6
    assert sum(s.weight for s in found) == pytest.approx(
        sum(s.weight for s in segments)
    )
    assert [s.at for s in found if s.category == "person"] == [
        50.0,
        150.0,
        250.0,
        350.0,
        450.0,
        550.0,
    ]
    mixed = [s for s in found if s.mix]
    assert mixed and all(set(s.mix) <= {"git", "read and search"} for s in mixed)
