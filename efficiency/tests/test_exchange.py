"""Export and import between machines, and the path rules that Windows needs."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import cli, exchange, paths, store


def seed(db: sqlite3.Connection, host: str) -> None:
    db.execute(
        "INSERT INTO requests (host, provider, transcript, message, time) VALUES "
        "(?, 'claude', 't', 'm1', '2026-11-10T10:00:00Z'), "
        "(?, 'claude', 't', 'm2', '2026-11-14T10:00:00Z')",
        (host, host),
    )
    db.execute(
        "INSERT INTO cpu_minutes (host, time, seconds, cores, cpu_seconds) "
        "VALUES (?, '2026-11-14T10:00:00Z', 60, 8, 30)",
        (host,),
    )
    db.execute(
        "INSERT INTO checks VALUES ('2026-11-14', ?, 'ci', 'rust', 'clippy', "
        "'2026-11-14T10:00:00Z', 1, 1, 0, 0, 5, 4, 5)",
        (host,),
    )
    db.commit()


def dump(db: sqlite3.Connection, host: str) -> dict[str, list[tuple[object, ...]]]:
    return {
        name: sorted(
            db.execute(f'SELECT * FROM "{name}" WHERE host = ?', (host,)).fetchall(),
            key=repr,
        )
        for name in exchange.host_tables(db)
    }


@pytest.fixture
def source(tmp_path: Path) -> sqlite3.Connection:
    db = store.open_store(tmp_path / "source.sqlite")
    seed(db, "cygnus")
    seed(db, "other")
    return db


def test_export_then_import_reproduces_the_hosts_rows(
    tmp_path: Path, source: sqlite3.Connection
) -> None:
    out = tmp_path / "e.gz"
    counts = exchange.export(source, "cygnus", None, out)
    assert counts["requests"] == 2 and counts["cpu_minutes"] == 1
    target = store.open_store(tmp_path / "target.sqlite")
    found = exchange.import_file(target, out, "jarvis")
    assert found.host == "cygnus" and found.added["requests"] == 2
    assert dump(target, "cygnus") == dump(source, "cygnus")
    assert dump(target, "other") == {name: [] for name in dump(target, "other")}


def test_importing_twice_or_overlapping_changes_nothing(
    tmp_path: Path, source: sqlite3.Connection
) -> None:
    whole = tmp_path / "whole.gz"
    recent = tmp_path / "recent.gz"
    exchange.export(source, "cygnus", None, whole)
    exchange.export(source, "cygnus", "2026-11-12", recent)
    target = store.open_store(tmp_path / "target.sqlite")
    exchange.import_file(target, recent, "jarvis")
    exchange.import_file(target, whole, "jarvis")
    before = dump(target, "cygnus")
    again = exchange.import_file(target, whole, "jarvis")
    exchange.import_file(target, recent, "jarvis")
    assert again.added == {} and again.seen["requests"] == 2
    assert dump(target, "cygnus") == before == dump(source, "cygnus")


def test_since_filters_by_each_tables_time(
    tmp_path: Path, source: sqlite3.Connection
) -> None:
    counts = exchange.export(source, "cygnus", "2026-11-12", tmp_path / "e.gz")
    assert counts["requests"] == 1


def test_a_fuller_total_replaces_a_smaller_one(
    tmp_path: Path, source: sqlite3.Connection
) -> None:
    out = tmp_path / "e.gz"
    target = store.open_store(tmp_path / "target.sqlite")
    exchange.export(source, "cygnus", None, out)
    exchange.import_file(target, out, "jarvis")
    source.execute("UPDATE checks SET runs = 3, seconds = 15 WHERE host = 'cygnus'")
    exchange.export(source, "cygnus", None, out)
    exchange.import_file(target, out, "jarvis")
    assert target.execute("SELECT runs, seconds FROM checks").fetchall() == [(3, 15.0)]


def test_a_file_from_this_host_or_a_newer_schema_is_refused(
    tmp_path: Path, source: sqlite3.Connection
) -> None:
    out = tmp_path / "e.gz"
    exchange.export(source, "cygnus", None, out)
    target = store.open_store(tmp_path / "target.sqlite")
    with pytest.raises(exchange.ExchangeError, match="this host"):
        exchange.import_file(target, out, "cygnus")
    import gzip

    newer = tmp_path / "newer.gz"
    with gzip.open(newer, "wt") as handle:
        handle.write(json.dumps({"export": 1, "host": "x", "schema": 999}) + "\n")
    with pytest.raises(exchange.ExchangeError, match="newer"):
        exchange.import_file(target, newer, "jarvis")


def test_a_damaged_or_hostile_file_changes_nothing(
    tmp_path: Path, source: sqlite3.Connection
) -> None:
    import gzip

    bad = tmp_path / "bad.gz"
    good = {
        "table": "requests",
        "row": {
            "host": "x",
            "provider": "p",
            "transcript": "t",
            "message": "m",
            "time": "z",
        },
    }
    evil = {"table": "beads", "row": {"host": "x", "id": "b"}}
    with gzip.open(bad, "wt") as handle:
        handle.write(json.dumps({"export": 1, "host": "x", "schema": 1}) + "\n")
        handle.write(json.dumps(good) + "\n" + json.dumps(evil) + "\n")
    target = store.open_store(tmp_path / "target.sqlite")
    with pytest.raises(exchange.ExchangeError, match="unknown table"):
        exchange.import_file(target, bad, "jarvis")
    assert target.execute("SELECT count(*) FROM requests").fetchone() == (0,)


def test_the_commands_move_rows_and_coverage_lists_each_hosts_newest_day(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(paths, "host", lambda: "cygnus")
    db = store.open_store(tmp_path / "s.sqlite")
    seed(db, "cygnus")
    out = tmp_path / "x.gz"
    exchange.export(db, "cygnus", None, out)
    monkeypatch.setattr(paths, "host", lambda: "jarvis")
    monkeypatch.setattr(cli, "sources", lambda: [])
    assert cli.main(["import", str(out)]) == 0
    assert cli.main(["import", str(out)]) == 0
    capsys.readouterr()
    assert cli.main(["coverage", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert found["hosts"]["cygnus"] == {"newest": "2026-11-14", "no_rows": ["leases"]}


def test_project_folders_and_working_directories_on_either_platform() -> None:
    assert paths.project_name("/Users/a/beryl") == "-Users-a-beryl"
    assert paths.project_name("C:\\Users\\a\\beryl") == "C--Users-a-beryl"
    assert paths.inside("/Users/a/beryl/.worktrees/x", "/Users/a/beryl")
    assert not paths.inside("/Users/a/beryl2", "/Users/a/beryl")
    assert paths.inside("c:\\Users\\a\\beryl\\.worktrees\\x", "C:\\Users\\a\\beryl")
    assert paths.inside("C:/Users/a/beryl", "C:\\Users\\a\\beryl")
    assert not paths.inside("C:\\Users\\a\\beryl-old", "C:\\Users\\a\\beryl")
