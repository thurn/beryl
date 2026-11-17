"""Interrupted processes cannot become flaky tests, verdicts or bisection culprits."""

from __future__ import annotations

import os
import signal
import sys
from datetime import datetime, timezone
from pathlib import Path

import pytest

from beryl_tools import bisect, checks, runlog, step_results, step_runner, system
from beryl_tools.clock import Clock


@pytest.mark.parametrize(
    ("code", "windows", "kind", "number"),
    [
        (0, False, "pass", None),
        (1, False, "fail", None),
        (125, False, "cannot-run", None),
        (-15, False, "external", 15),
        (137, False, "external", 9),
        (None, False, "stopped", None),
        (101, False, "fail", None),
        (137, True, "fail", None),
        (0xC000013A, True, "external", None),
        (-1073741510, True, "external", None),
    ],
)
def test_exit_semantics(
    monkeypatch: pytest.MonkeyPatch,
    code: int | None,
    windows: bool,
    kind: str,
    number: int | None,
) -> None:
    monkeypatch.setattr(system, "WINDOWS", windows)
    assert step_results.kind(code) == kind
    assert step_results.signal_number(code) == number


@pytest.mark.parametrize(
    ("ending", "kind", "code"),
    [
        ("raise AssertionError('broken')", "fail", 1),
        ("sys.exit(125)", "cannot-run", 125),
        pytest.param(
            "os.kill(os.getpid(), signal.SIGTERM)",
            "external",
            -signal.SIGTERM,
            marks=pytest.mark.skipif(system.WINDOWS, reason="POSIX signal status"),
        ),
        ("sys.exit(137)", "fail" if system.WINDOWS else "external", 137),
    ],
)
def test_subprocess_results_retain_their_own_evidence(
    ending: str,
    kind: str,
    code: int,
    tmp_path: Path,
) -> None:
    # A complete named result before an interruption must not hide the incomplete step.
    script = (
        "import os, signal, sys, json\n"
        "print('own step output', flush=True)\n"
        "with open(os.environ['BERYL_CHECK_RESULTS'], 'a') as out:\n"
        " out.write(json.dumps({'check':'probe:finished', 'outcome':'pass', 'seconds':0})+'\\n')\n"
        + ending
    )
    lane = checks.Lane("probe").step(
        "tests",
        [sys.executable, "-c", script],
        tmp_path,
        rerun=checks.whole,
    )
    report = checks.run_all([lane], "ci", 60)
    assert report.code == 1
    [outcome] = report.outcomes
    assert not outcome.ok and not outcome.flaky
    assert bool(outcome.failure) == (kind == "fail")
    assert bool(bisect.suspects(report.outcomes)) == (kind == "fail")
    records = runlog.read_log(datetime.min.replace(tzinfo=timezone.utc))
    assert {r["result_kind"] for r in records} == {kind}
    assert {r["exit_code"] for r in records} == {code}
    if kind != "fail":
        assert {r["timeout"] for r in records} == {kind}
        assert len(records) == 2  # No rerun; partial pass plus incomplete whole step.
    assert "own step output" in Path(str(records[0]["output_path"])).read_text()


def test_a_missing_executable_is_cannot_run(tmp_path: Path) -> None:
    lane = checks.Lane("probe").step("missing", [str(tmp_path / "missing")])
    assert checks.run_all([lane], "ci", 60).code == 1
    [record] = runlog.read_log(datetime.min.replace(tzinfo=timezone.utc))
    assert record["result_kind"] == "cannot-run"
    assert record["exit_code"] == 125
    assert "Could not start step" in Path(str(record["output_path"])).read_text()


class AdvancedClock(Clock):
    """The subprocess gets a zero remaining limit, independent of machine speed."""

    def now(self) -> float:
        return 10.0

    def wall(self) -> float:
        return 10.0


@pytest.mark.parametrize(
    ("script", "wall_limit", "limit", "kind", "passed"),
    [
        ("ci", None, 0.0, "hang", False),
        ("ci", 0.0, 100.0, "wall", False),
        ("check", 0.0, 100.0, "deferred", True),
    ],
)
def test_hang_and_deferral_have_distinct_records(
    script: str,
    wall_limit: float | None,
    limit: float,
    kind: str,
    passed: bool,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    runner = step_runner.Steps(AdvancedClock(), wall_limit=wall_limit)
    limits = checks.hang.Limits(script)
    monkeypatch.setattr(limits, "limit", lambda *_: (limit, "injected limit"))
    lane = checks.Lane("probe").step("tests", [sys.executable, "-c", "pass"], tmp_path)
    run = runlog.Run(script)
    outcome = checks.run_lane(lane, run, dict(os.environ), runner, limits)
    assert outcome.ok == passed
    assert outcome.hung == (kind == "hang")
    assert {r["result_kind"] for r in run.logged} == {kind}
    assert not outcome.failure
    assert outcome.deferred == (["tests"] if kind == "deferred" else [])


@pytest.mark.parametrize("code", [125, -15, 137])
@pytest.mark.skipif(system.WINDOWS, reason="POSIX signal status")
def test_an_incomplete_probe_cannot_blame_a_commit(code: int, tmp_path: Path) -> None:
    ending = "os.kill(os.getpid(), signal.SIGTERM)" if code < 0 else f"sys.exit({code})"
    step = checks.Step(
        "tests", [sys.executable, "-c", "import os, signal, sys; " + ending], tmp_path
    )
    failure = checks.Failure(step, [], [{"check": "probe:tests", "outcome": "fail"}])
    suspect = bisect.Suspect("probe", failure, None, failure.records)
    tree = bisect.Tree(tmp_path)
    tree.path = tmp_path
    probe = bisect.Prober(tree, step_runner.Steps())
    assert probe.step(runlog.Run("bisect"), [suspect], False) == [(suspect, None)]


@pytest.mark.skipif(system.WINDOWS, reason="POSIX signal status")
def test_pending_results_cannot_excuse_an_externally_terminated_step(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(checks.verdicts, "pending_checks", lambda: {"probe:pending"})
    script = (
        "import json, os, signal\n"
        "with open(os.environ['BERYL_CHECK_RESULTS'], 'a') as out:\n"
        " out.write(json.dumps({'check':'probe:pending', 'outcome':'fail', 'seconds':0})+'\\n')\n"
        "os.kill(os.getpid(), signal.SIGTERM)"
    )
    lane = checks.Lane("probe").step(
        "tests", [sys.executable, "-c", script], tmp_path, rerun=checks.whole
    )
    report = checks.run_all([lane], "check", 60)
    assert report.code == 1
    [outcome] = report.outcomes
    assert not outcome.ok and not outcome.excused and not outcome.flaky
    assert not outcome.failure
    [record] = runlog.read_log(datetime.min.replace(tzinfo=timezone.utc))
    assert record["outcome"] == "pending-fail"
    assert record["timeout"] == "external"
