"""The command: `ingest`, `sql` and `coverage` against a store in a temporary cache."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from beryl_efficiency import cli, ingest, paths, store
from beryl_efficiency.beads import Beads
from tests.synthetic import bead, listing

# The tables "The store" in docs/efficiency-dashboard.md lists, and those that serve them:
# labels for the beads table, ingests for the tool's own cost, and the four that bead
# windows are derived from.
TABLES = {
    "requests",
    "tool_calls",
    "windows",
    "beads",
    "labels",
    "commits",
    "checks",
    "lane_runs",
    "verdicts",
    "cpu",
    "cpu_minutes",
    "leases",
    "sources",
    "ingests",
    "sessions",
    "codex_marks",
    "prompts",
    "settings",
    "window_time",
    "window_dollars",
    "transcripts",
    "enact_events",
    "tool_results",
}


@pytest.fixture(autouse=True)
def beads(monkeypatch: pytest.MonkeyPatch) -> None:
    output = listing(bead("by-a", labels=["tier:opus"]), bead("by-a.1", parent="by-a"))
    sources: list[ingest.Source] = [Beads(lambda: output)]
    monkeypatch.setattr(cli, "sources", lambda: sources)


def run(capsys: pytest.CaptureFixture[str], *argv: str) -> str:
    assert cli.main(list(argv)) == 0
    return capsys.readouterr().out


def test_ingest_then_count_the_beads(capsys: pytest.CaptureFixture[str]) -> None:
    run(capsys, "ingest")
    assert run(capsys, "sql", "--no-ingest", "select count(*) from beads").split() == [
        "count(*)",
        "2",
    ]
    second = json.loads(run(capsys, "ingest", "--json"))
    assert second["bytes_read"] == 0 and second["sources"]["beads"]["rows"] == 0


def test_a_query_ingests_a_stale_store_first(
    capsys: pytest.CaptureFixture[str],
) -> None:
    found = json.loads(run(capsys, "sql", "--json", "select id from beads order by id"))
    assert found["rows"] == [["by-a"], ["by-a.1"]]
    # Fresh now: the next query reads the store as it is.
    assert not ingest.stale(paths.store(), cli.MAX_AGE)


def test_sql_cannot_write(capsys: pytest.CaptureFixture[str]) -> None:
    run(capsys, "ingest")
    assert cli.main(["sql", "--no-ingest", "delete from beads"]) == 1
    assert "readonly" in capsys.readouterr().err
    assert "2" in run(capsys, "sql", "--no-ingest", "select count(*) from beads")


def test_coverage_reports_rows_skips_and_its_own_cost(
    capsys: pytest.CaptureFixture[str],
) -> None:
    run(capsys, "ingest")
    run(capsys, "ingest")
    found = json.loads(run(capsys, "coverage", "--json", "--no-ingest"))
    assert set(found["tables"]) == TABLES
    assert found["tables"]["beads"] == 2 and found["tables"]["labels"] == 1
    assert found["skipped"] == {"beads": 0}
    assert found["ingest_cost"]["count"] == 2
    assert [i["bytes_read"] > 0 for i in found["ingests"]] == [False, True]
    text = run(capsys, "coverage", "--no-ingest")
    assert (
        "wall" in text
        and "cpu" in text
        and text.rstrip().splitlines()[-1].startswith("more: ")
    )


def test_an_empty_store_answers(capsys: pytest.CaptureFixture[str]) -> None:
    found = json.loads(run(capsys, "coverage", "--json", "--no-ingest"))
    assert found["tables"]["beads"] == 0 and found["ingest_cost"]["count"] == 0


def test_a_store_with_a_newer_schema_is_refused(tmp_path: Path) -> None:
    path = tmp_path / "store.sqlite"
    db = store.open_store(path)
    db.execute(f"PRAGMA user_version = {store.version() + 1}")
    db.close()
    with pytest.raises(RuntimeError, match="newer"):
        store.open_store(path)


def test_the_listing_names_every_query(capsys: pytest.CaptureFixture[str]) -> None:
    listed = run(capsys)
    for name in cli.QUERIES:
        assert f"  {name} " in listed
