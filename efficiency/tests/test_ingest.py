"""Ingest reads each source once: from recorded offsets for files of lines, and only when it
changed for a command's output."""

from __future__ import annotations

import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import ingest, store
from beryl_efficiency.beads import Beads
from beryl_efficiency.ingest import SourceError
from tests.synthetic import Lines, bead, listing


def open_db(tmp_path: Path) -> sqlite3.Connection:
    return store.open_store(tmp_path / "store.sqlite")


def priorities(db: sqlite3.Connection) -> dict[str, int]:
    return {str(r[0]): int(r[1]) for r in db.execute("SELECT id, priority FROM beads")}


def test_lines_are_read_once_and_a_partial_line_waits(tmp_path: Path) -> None:
    folder = tmp_path / "logs"
    folder.mkdir()
    log = folder / "a.jsonl"
    log.write_bytes(b'{"id": "x", "n": 1}\n{"id": "y", "n": 2}\n{"id": "z"')
    db = open_db(tmp_path)
    source = Lines(folder)

    first = ingest.ingest(db, [source], "host").sources["lines"]
    assert priorities(db) == {"x": 1, "y": 2}
    assert first.rows == 2 and first.bytes_read == log.read_bytes().rfind(b"\n") + 1

    again = ingest.ingest(db, [source], "host").sources["lines"]
    assert (again.bytes_read, again.rows) == (0, 0)

    # The writer finishes the last line: only it is read.
    with open(log, "ab") as handle:
        handle.write(b', "n": 3}\n')
    third = ingest.ingest(db, [source], "host").sources["lines"]
    assert third.rows == 1 and third.bytes_read == len(b'{"id": "z", "n": 3}\n')
    assert priorities(db) == {"x": 1, "y": 2, "z": 3}


def test_malformed_lines_are_skipped_and_counted(tmp_path: Path) -> None:
    folder = tmp_path / "logs"
    folder.mkdir()
    (folder / "a.jsonl").write_bytes(
        b'{"id": "x", "n": 1}\nnot json\n{"id": "y"}\n\n{"id": "z", "n": 3}\n'
    )
    db = open_db(tmp_path)
    tally = ingest.ingest(db, [Lines(folder)], "host").sources["lines"]
    assert (tally.rows, tally.skipped) == (2, 2)
    assert priorities(db) == {"x": 1, "z": 3}
    assert db.execute("SELECT skipped FROM sources").fetchone()[0] == 2


def test_a_replaced_file_is_read_again_from_the_start(tmp_path: Path) -> None:
    folder = tmp_path / "logs"
    folder.mkdir()
    log = folder / "a.jsonl"
    log.write_bytes(b'{"id": "x", "n": 1}\n{"id": "y", "n": 2}\n')
    db = open_db(tmp_path)
    ingest.ingest(db, [Lines(folder)], "host")
    # Rotated: new first bytes, longer than the old offset.
    log.write_bytes(b'{"id": "w", "n": 7}\n{"id": "x", "n": 5}\n{"id": "v", "n": 6}\n')
    tally = ingest.ingest(db, [Lines(folder)], "host").sources["lines"]
    assert tally.rows == 3
    assert priorities(db) == {"w": 7, "x": 5, "y": 2, "v": 6}
    # Truncated to fewer bytes than were read.
    log.write_bytes(b'{"id": "u", "n": 4}\n')
    ingest.ingest(db, [Lines(folder)], "host")
    assert priorities(db)["u"] == 4


def test_a_snapshot_is_written_only_when_it_changed(tmp_path: Path) -> None:
    output = listing(
        bead("by-a", labels=["tier:opus"]),
        bead(
            "by-a.1", parent="by-a", status="closed", closed_at="2026-11-02T10:00:00Z"
        ),
    )
    db = open_db(tmp_path)
    source = Beads(lambda: output)
    first = ingest.ingest(db, [source], "host").sources["beads"]
    assert first.bytes_read == len(output)
    assert ingest.ingest(db, [source], "host").sources["beads"].bytes_read == 0

    # by-a.1 is gone and by-a changed its labels: the store follows.
    output = listing(bead("by-a", labels=["tier:sonnet", "lane:x"]))
    ingest.ingest(db, [source], "host")
    assert [r[0] for r in db.execute("SELECT id FROM beads")] == ["by-a"]
    assert sorted(r[0] for r in db.execute("SELECT label FROM labels")) == [
        "lane:x",
        "tier:sonnet",
    ]


def test_a_source_that_fails_is_reported_and_others_still_run(tmp_path: Path) -> None:
    def broken() -> bytes:
        raise SourceError("bd: not found")

    folder = tmp_path / "logs"
    folder.mkdir()
    (folder / "a.jsonl").write_bytes(b'{"id": "x", "n": 1}\n')
    db = open_db(tmp_path)
    summary = ingest.ingest(db, [Beads(broken), Lines(folder)], "host")
    assert summary.sources["beads"].error == "bd: not found"
    assert summary.sources["lines"].rows == 1
    nonsense = ingest.ingest(db, [Beads(lambda: b"{")], "host")
    assert nonsense.sources["beads"].error.startswith("bd printed no JSON")


def test_every_ingest_records_its_own_cost(tmp_path: Path) -> None:
    db = open_db(tmp_path)
    ingest.ingest(db, [Beads(lambda: listing(bead("by-a")))], "here")
    ingest.ingest(db, [Beads(lambda: listing(bead("by-a")))], "here")
    rows = db.execute(
        "SELECT host, wall_seconds >= 0, cpu_seconds >= 0, bytes_read FROM ingests "
        "ORDER BY id"
    ).fetchall()
    assert [(r[0], r[1], r[2]) for r in rows] == [("here", 1, 1)] * 2
    assert rows[1][3] == 0


def test_rebuild_recreates_the_store(tmp_path: Path) -> None:
    path = tmp_path / "store.sqlite"
    lock = tmp_path / "ingest.lock"
    source = Beads(lambda: listing(bead("by-a"), bead("by-b")))
    ingest.run(path, lock, [source], "host")
    again = ingest.run(path, lock, [source], "host")
    assert again is not None and again.total().bytes_read == 0
    rebuilt = ingest.run(path, lock, [source], "host", rebuild=True)
    assert rebuilt is not None
    assert rebuilt.total().rows == 2 and rebuilt.total().bytes_read > 0


def test_an_ingest_that_need_not_wait_leaves_a_held_store_alone(tmp_path: Path) -> None:
    """A query's ingest does not wait behind another ingest, such as the page server's: it
    answers from the store as it is."""
    path = tmp_path / "store.sqlite"
    lock = tmp_path / "ingest.lock"
    source = Beads(lambda: listing(bead("by-a")))
    with store.locked(lock) as held:
        assert held
        assert ingest.run(path, lock, [source], "host", wait=False) is None
    assert not path.exists()
    done = ingest.run(path, lock, [source], "host", wait=False)
    assert done is not None and done.total().rows == 1


def test_a_snapshot_counts_its_malformed_records_once(tmp_path: Path) -> None:
    db = open_db(tmp_path)
    for title in ("one", "two"):
        output = listing(bead("by-a", title=title), {"id": 7}, {"title": "no id"})
        tally = ingest.ingest(db, [Beads(lambda: output)], "host").sources["beads"]
        assert tally.skipped == 2
    assert db.execute("SELECT skipped FROM sources").fetchone()[0] == 2


def test_files_read_by_an_ingest_stopped_before_finish_are_finished_next_time(
    tmp_path: Path,
) -> None:
    """An ingest killed after committing a file's rows but before `finish` derived what
    depends on them must leave that file to the next ingest's `finish`, although its
    offset says it has been read."""

    class Finishing(Lines):
        def __init__(self, folder: Path) -> None:
            super().__init__(folder)
            self.finished: list[list[str]] = []
            self.stop = True

        def finish(self, db: sqlite3.Connection, changed: list[Path]) -> int:
            self.finished.append([p.name for p in changed])
            if self.stop:
                raise RuntimeError("killed")
            return 0

    folder = tmp_path / "logs"
    folder.mkdir()
    (folder / "a.jsonl").write_bytes(b'{"id": "x", "n": 1}\n')
    db = open_db(tmp_path)
    source = Finishing(folder)
    with pytest.raises(RuntimeError):
        ingest.ingest(db, [source], "host")
    source.stop = False
    assert ingest.ingest(db, [source], "host").total().bytes_read == 0
    ingest.ingest(db, [source], "host")
    assert source.finished == [["a.jsonl"], ["a.jsonl"], []]
