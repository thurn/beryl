"""The fast check per push and the cost of a slow-CI failure, over small synthetic sources:
a session's `enact push` calls split by the run log, and a Steward ticket's failure costed from
slow CI's runs and bisection."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from beryl_efficiency import cilog, claude, cli, ingest
from tests.legacy_sources import Beads, RunLog, Verdicts, runlog_path
from beryl_efficiency.failures import (
    BisectRun,
    CiRun,
    FailedCheck,
    StewardCost,
    failure,
)
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record
from tests.test_ci import check_bead, line, verdict

HOST = "jarvis"
ENACT = "python3 ~/enact/scripts/enact.py"

pytestmark: pytest.MarkDecorator = pytest.mark.usefixtures("utc_days")


def at(hour: int, minute: int, second: int = 0) -> str:
    return f"2026-11-15T{hour:02d}:{minute:02d}:{second:02d}.000Z"


BEADS: bytes = listing(
    bead_record(
        "by-p.1",
        status="closed",
        started_at=at(10, 0),
        closed_at=at(10, 30),
        metadata={
            "enact_worktree": "/work/beryl/.worktrees/by-p.1",
            "enact_spawn_host": HOST,
        },
    ),
)
EXPORT: str = check_bead(
    "rust:beryl_cli::a::b",
    "rust",
    verdict("v1", at(10, 12), "catch", script="check", lane="rust"),
    verdict("v2", at(10, 12), "flaky", script="ci", lane="rust"),
)
OUTPUT = (
    "check: rust 90 s\n"
    "rust-tests: 12 tests deferred to slow CI: they do not fit the 20 s left of the fast"
    " check's target\n"
    "deferred: shots capture\n"
)


@pytest.fixture
def root(tmp_path: Path, cache: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """One ticket pushes twice. The first push, 10:10 to 10:15, runs the fast check, loses the
    race to staging and checks again on a new commit. The second runs in the background from
    10:20 until the session is told at 10:24 that it finished, and its run-log lines are from
    before the run log recorded worktrees."""
    transcript = (
        Transcript(tmp_path / "projects" / PROJECT / "t1.jsonl")
        .bash(at(10, 0, 1), "m1", "c1", f"{ENACT} start by-p.1")
        .result(at(10, 0, 2), "c1", '{"bead": "by-p.1"}')
        .bash(at(10, 10), "m2", "c2", f"{ENACT} push --bead by-p.1")
        .result(at(10, 15), "c2", stderr=OUTPUT)
        .bash(at(10, 20), "m3", "c3", f"{ENACT} push --bead by-p.1 > /tmp/push.log")
        .result(at(10, 20), "c3", "Command running in background with ID: b1.")
    )
    transcript.lines.append(
        json.dumps(
            {
                "type": "queue-operation",
                "operation": "enqueue",
                "timestamp": at(10, 24),
                "content": "<task-notification>\n<task-id>b1</task-id>\n"
                "<tool-use-id>c3</tool-use-id>\n<status>completed</status>",
            }
        )
    )
    (
        transcript.bash(at(10, 29), "m4", "c4", f"{ENACT} close by-p.1 --reason done")
        .result(at(10, 29, 30), "c4")
        .save()
    )
    log = runlog_path(cache)
    log.parent.mkdir(parents=True)
    lines = [
        # The first check: the rust lane from 10:10:30 to 10:12, names inside it.
        line(
            at(10, 12),
            "rust:beryl_cli::a::b",
            "pass",
            90,
            100,
            "check",
            None,
            "k1",
            "by-p.1",
        ),
        line(at(10, 11), "names:check", "pass", 20, 10, "check", None, "k1", "by-p.1"),
        # The repeat on the rebased commit: rust from 10:13:30 to 10:14.
        line(
            at(10, 14),
            "rust:beryl_cli::a::b",
            "pass",
            30,
            40,
            "check",
            None,
            "k2",
            "by-p.1",
        ),
        # Another worktree's check at the same time is not this push's.
        line(at(10, 12), "repo:lint", "pass", 60, 60, "check", None, "x1", "by-q.2"),
        # The background push's check, with no worktree.
        line(at(10, 23), "rust:beryl_cli::a::b", "pass", 60, 50, "check", None, "k3"),
    ]
    log.write_text("".join(l + "\n" for l in lines))
    sources: list[ingest.Source] = [
        Beads(lambda: BEADS),
        claude.Transcripts(tmp_path / "projects", CHECKOUT, HOST),
        RunLog(log),
        Verdicts(lambda: EXPORT.encode()),
    ]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    assert cli.main(["ingest"]) == 0
    return tmp_path


def query(capsys: pytest.CaptureFixture[str], *argv: str) -> dict[str, object]:
    capsys.readouterr()
    assert cli.main([*argv, "--since", "all", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert isinstance(found, dict)
    return found


def rows_of(found: dict[str, object], key: str = "rows") -> list[dict[str, object]]:
    rows = found[key]
    assert isinstance(rows, list)
    return rows


def test_a_push_is_split_into_its_check_lanes_and_the_rest(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    found = query(capsys, "pushes")
    background, first = rows_of(found)
    assert first["seconds"] == 300.0
    # Covered by a lane run: 10:10:30 to 10:12 and 10:13:30 to 10:14.
    assert first["fast_check_seconds"] == 120.0
    assert first["rest_seconds"] == 180.0
    fast, rest = first["fast_check_seconds"], first["rest_seconds"]
    assert isinstance(fast, float) and isinstance(rest, float)
    assert fast + rest == first["seconds"]
    lanes = {str(l["lane"]): l for l in rows_of(first, "lanes")}
    assert set(lanes) == {"rust", "names"}
    assert lanes["rust"]["seconds"] == 120.0 and lanes["rust"]["runs"] == 2
    assert lanes["names"]["seconds"] == 20.0
    assert first["cpu_seconds"] == 150.0
    assert (first["checks"], first["repeats"]) == (2, 1)
    assert (first["repeat_seconds"], first["repeat_cpu_seconds"]) == (30.0, 40.0)
    assert first["deferred"] == ["rust-tests: 12 tests", "shots capture"]


def test_a_push_with_no_joinable_lines_is_not_split(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    background = rows_of(query(capsys, "pushes"))[0]
    # It ran in the background until the session was told it finished.
    assert background["seconds"] == 240.0
    assert background["split"] is False
    assert background["fast_check_seconds"] is None
    assert background["rest_seconds"] is None


def test_the_fast_check_view_adds_up_pushes(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    found = query(capsys, "pushes")
    assert (found["pushes"], found["split"]) == (2, 1)
    assert (found["repeats"], found["repeat_seconds"]) == (1, 30.0)
    assert found["deferred"] == [
        {"name": "rust-tests: 12 tests", "count": 1},
        {"name": "shots capture", "count": 1},
    ]
    # Only the fast check's own failure counts before a push.
    failures = found["failures"]
    assert isinstance(failures, dict)
    assert (failures["catch"], failures["flaky"]) == (1, 0)
    capsys.readouterr()
    assert cli.main(["pushes", "--since", "all", "--no-ingest"]) == 0
    text = capsys.readouterr().out
    assert "not split" not in text  # The table shows `-` for an unsplit push.
    assert "repeats 1 (30 s, 40 cpu s)" in text


def test_the_bead_query_shows_its_pushes(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    found = query(capsys, "bead", "by-p.1")
    assert [p["fast_check_seconds"] for p in rows_of(found, "pushes")] == [120.0, None]
    capsys.readouterr()
    assert cli.main(["bead", "by-p.1", "--no-ingest"]) == 0
    text = capsys.readouterr().out
    assert "fast check 120 (rust 120, names 20) | rebase and push 180" in text
    assert "240 s, not split" in text


def test_the_push_metrics(root: Path, capsys: pytest.CaptureFixture[str]) -> None:
    walls = rows_of(query(capsys, "metrics", "push.wall-seconds"), "series")
    assert [(p["count"], p["median"]) for p in walls] == [(2, 270.0)]
    repeats = rows_of(query(capsys, "metrics", "push.repeats"), "series")
    assert [(p["count"], p["median"]) for p in repeats] == [(2, 0.5)]


RUNS = [
    CiRun("jarvis", "c8", at(9, 0), False),
    CiRun("jarvis", "c9abcdef", at(10, 0), True),
    CiRun("cygnus", "c9abcdef", at(10, 40), True),
    CiRun("jarvis", "d1", at(11, 30), False),
]
BISECT = [
    BisectRun("c5", at(10, 5), 600.0),
    BisectRun("zz", at(10, 6), 120.0),
]
PAID = StewardCost(4.5, 30.0, 20.0)


def test_a_failure_costs_its_steward_its_bisection_and_its_time_behind() -> None:
    checks = [FailedCheck("rust:a", "rust", "catch"), FailedCheck("ui:b", "ui", "miss")]
    found = failure(
        "by-s",
        "Fix slow CI",
        "closed",
        "c1..c9",
        at(10, 10),
        RUNS,
        BISECT,
        {"c5", "c9abcdef"},
        checks,
        PAID,
    )
    assert found.failed_at == at(10, 0) and found.host == "jarvis"
    assert found.dollars == 4.5 and found.agent_minutes == 30.0
    # The Steward's 20 core minutes and bisection's 600 CPU seconds in the range.
    assert (found.steward_core_minutes, found.bisect_core_minutes) == (20.0, 10.0)
    assert found.core_minutes == 30.0
    # From the failing run's end to the next passing run's end.
    assert found.behind_minutes == 90.0
    assert found.checks == checks


def test_a_failure_the_run_log_did_not_see_has_no_time_behind() -> None:
    found = failure(
        "by-s",
        None,
        "open",
        "c1..e7",
        at(12, 0),
        RUNS,
        BISECT,
        set(),
        [],
        StewardCost(None, None, None),
    )
    assert found.failed_at == at(12, 0) and found.host is None
    # The Steward ran on a host with no data: its cost is unknown, not zero.
    assert found.no_data and found.dollars is None and found.agent_minutes is None
    assert found.behind_minutes is None
    # No commits for the range: bisection after the failure counts, of which there is none.
    assert found.bisect_core_minutes == 0.0
    assert found.core_minutes == 0.0 and found.steward_core_minutes is None
