"""The fast check's Beryl 3D `cargo-test` step runs the workspace tests that fit the time left
(beryl3d/test_fit.py)."""

import subprocess
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import runlog, rust_fit, tokens
from beryl_tools.beryl3d import test_fit
from beryl_tools.selection import Record


class Known:
    def __init__(self, records: dict[str, Record]) -> None:
        self.records = records

    def get(self) -> dict[str, Record]:
        return self.records


JUNIT = """<testsuites><testsuite>
<testcase classname="beryl_scene" name="pose::a_new_test" time="0.5"/>
<testcase classname="beryl_scene" name="pose::cheap" time="1.0"/>
</testsuite></testsuites>"""


def test_only_the_tests_that_fit_run_and_the_rest_go_to_the_sweep(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    logged: list[tuple[str, str, float]] = []
    ran: list[list[str]] = []
    junit: Path = tmp_path / "junit.xml"
    monkeypatch.setattr(test_fit, "JUNIT", junit)
    monkeypatch.setattr(runlog, "result", lambda c, o, s, *_: logged.append((c, o, s)))
    # 55 s before the hard limit, less the step's 20 s margin, leaves 35 s to plan.
    monkeypatch.setattr(test_fit.deadline, "until_hard_limit", lambda: 55.0)
    monkeypatch.setattr(
        test_fit.tokens,
        "acquire",
        lambda *_a, **_k: tokens.Lease(2, [], klass="push", wanted=12),
    )
    monkeypatch.setattr(
        rust_fit,
        "listed",
        lambda *_a: [
            "beryl_scene pose::cheap",
            "beryl_scene pose::slow",
            "beryl_scene pose::a_new_test",
        ],
    )

    def run(command: list[str], **_kwargs: Any) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        junit.write_text(JUNIT)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(test_fit.subprocess, "run", run)
    known = Known(
        {
            "b3d-test:beryl_scene::pose::cheap": Record(0, 0, 10, 2.0),
            "b3d-test:beryl_scene::pose::slow": Record(0, 0, 10, 200.0),
        }
    )
    assert test_fit.run(known) == 0  # pyre-ignore[6]: a stand-in for test_fit.Known
    # 35 s on 2 threads: the untimed test (3 s) and the cheap one (2 s) fit, the 200 s one
    # does not.
    [command] = ran
    expression = command[command.index("-E") + 1]
    assert "test(=pose::cheap)" in expression
    assert "test(=pose::a_new_test)" in expression
    assert "slow" not in expression
    assert ("b3d-test:beryl_scene::pose::slow", "skip", 0.0) in logged
    assert ("b3d-test:beryl_scene::pose::cheap", "pass", 1.0) in logged
    assert "1 tests deferred to the Beryl 3D sweep" in capsys.readouterr().out


def test_outside_a_fast_check_every_test_runs_and_is_logged(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.delenv(test_fit.deadline.STARTED_ENV, raising=False)
    assert test_fit.known() is None
    logged: list[tuple[str, str, float]] = []
    ran: list[list[str]] = []
    junit: Path = tmp_path / "junit.xml"
    monkeypatch.setattr(test_fit, "JUNIT", junit)
    monkeypatch.setattr(runlog, "result", lambda c, o, s, *_: logged.append((c, o, s)))
    monkeypatch.setattr(
        test_fit.tokens, "acquire", lambda *_a, **_k: tokens.Lease(4, [])
    )

    def run(command: list[str], **_kwargs: Any) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        junit.write_text(JUNIT)
        return subprocess.CompletedProcess(command, 1)

    monkeypatch.setattr(test_fit.subprocess, "run", run)
    assert test_fit.run(None) == 1
    [command] = ran
    assert command[command.index("-E") + 1] == "all()"
    assert len(logged) == 2


def test_a_test_timed_in_both_lanes_counts_the_record_with_more_runs() -> None:
    fast = Known({"b3d-test:a::t": Record(0, 0, 2, 9.0)})
    sweep = Known(
        {"b3d-test:a::t": Record(0, 0, 5, 1.0), "b3d-test:a::u": Record(0, 0, 1, 2.0)}
    )
    both = test_fit.Known.__new__(test_fit.Known)
    both.lanes = [fast, sweep]  # pyre-ignore[8]: stand-ins for deadline.Records
    assert both.get() == {
        "b3d-test:a::t": Record(0, 0, 5, 1.0),
        "b3d-test:a::u": Record(0, 0, 1, 2.0),
    }
