"""The fast check's Beryl 3D `cargo-test` step runs the workspace tests that fit the time left
(beryl3d/test_fit.py)."""

from __future__ import annotations

import json
import subprocess
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import runlog, runstore, rust_fit, tokens
from beryl_tools.beryl3d import test_fit
from beryl_tools.selection import Record


class Nextest:
    """A stand-in for nextest's process: it writes `junit` when it ends, after `seconds`, and
    is stopped by SIGTERM when the step's wait runs out first, writing `stopped` instead.
    """

    def __init__(
        self,
        ran: list[list[str]],
        junit: Path,
        report: str,
        code: int,
        seconds: float = 0.0,
        stopped: str = "",
    ) -> None:
        self.ran: list[list[str]] = ran
        self.junit: Path = junit
        self.report: str = report
        self.code: int = code
        self.seconds: float = seconds
        self.stopped: str = stopped
        self.signals: list[int] = []

    def __call__(self, command: list[str], **_kwargs: Any) -> Nextest:
        self.ran.append(command)
        return self

    def wait(self, timeout: float | None = None) -> int:
        if self.signals:
            self.junit.write_text(self.stopped)
            return 100
        if timeout is not None and timeout < self.seconds:
            raise subprocess.TimeoutExpired("nextest", timeout)
        self.junit.write_text(self.report)
        return self.code

    def send_signal(self, number: int) -> None:
        self.signals.append(number)


class Known:
    def __init__(
        self, records: dict[str, Record], failed: set[str] | None = None
    ) -> None:
        self.records = records
        self.failed: set[str] = failed or set()

    def get(self) -> dict[str, Record]:
        return self.records

    def failures(self) -> set[str]:
        return self.failed


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

    monkeypatch.setattr(test_fit.subprocess, "Popen", Nextest(ran, junit, JUNIT, 0))
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

    # Once the slow test has failed on this branch, the retry runs it instead of cutting it.
    ran.clear()
    logged.clear()
    known.failed = {"b3d-test:beryl_scene::pose::slow"}
    assert test_fit.run(known) == 0  # pyre-ignore[6]: a stand-in for test_fit.Known
    [command] = ran
    assert "test(=pose::slow)" in command[command.index("-E") + 1]
    assert ("b3d-test:beryl_scene::pose::slow", "skip", 0.0) not in logged
    out = capsys.readouterr().out
    assert "1 tests that failed in the last 3 h run first" in out


def test_a_test_that_failed_recently_runs_however_little_time_is_left() -> None:
    known = {
        "b3d-test:a::cheap": Record(0, 0, 10, 2.0),
        "b3d-test:a::slow": Record(0, 0, 10, 200.0),
        "b3d-test:a::other": Record(0, 0, 10, 4.0),
    }
    tests = ["a cheap", "a slow", "a other"]
    # 20 s on 2 threads: without the failure only the two cheap tests fit.
    assert rust_fit.plan(tests, [], known, 20.0, 2, "b3d-test")[2] == [
        "b3d-test:a::slow"
    ]
    # The failed one is kept, and its 100 s leave no room for the rest.
    chosen, _, cut = rust_fit.plan(
        tests, [], known, 20.0, 2, "b3d-test", {"b3d-test:a::slow", "b3d-test:a::gone"}
    )
    assert chosen == ["a slow"]
    assert sorted(cut) == ["b3d-test:a::cheap", "b3d-test:a::other"]


def test_recent_failures_are_those_of_this_branch_and_of_the_sweep(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    now: datetime = datetime.now(timezone.utc)
    hours = tmp_path / "hours"
    hours.mkdir()
    monkeypatch.setattr(runstore, "hours_dir", lambda: hours)
    monkeypatch.setattr(runstore, "legacy_path", lambda: tmp_path / "none.jsonl")

    def record(check: str, lane: str, place: str, **more: object) -> dict[str, object]:
        when = more.pop("time", now)
        assert isinstance(when, datetime)
        return {
            "time": when.isoformat(timespec="seconds"),
            "script": "check",
            "place": place,
            "lane": lane,
            "check": f"b3d-test:a::{check}",
            "outcome": "fail",
            "seconds": 1.0,
            **more,
        }

    records = [
        record("mine", "beryl3d-tests", "enact/mine"),
        record("theirs", "beryl3d-tests", "enact/theirs"),
        record("staging", "b3d-cargo-test", "/tree"),
        record("passed", "beryl3d-tests", "enact/mine", outcome="pass"),
        record("killed", "beryl3d-tests", "enact/mine", timeout="wall"),
        record("old", "beryl3d-tests", "enact/mine", time=now - timedelta(hours=5)),
    ]
    (hours / (now.strftime(runstore.HOUR) + ".jsonl")).write_text(
        "".join(json.dumps(r) + "\n" for r in records)
    )
    assert test_fit.recent_failures("enact/mine", now - test_fit.RECENT) == {
        "b3d-test:a::mine",
        "b3d-test:a::staging",
    }


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

    monkeypatch.setattr(test_fit.subprocess, "Popen", Nextest(ran, junit, JUNIT, 1))
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


# nextest's report when the step stopped it: one test passed, one failed by itself, one was
# aborted by the stop, and `pose::late` never started.
STOPPED = """<testsuites><testsuite>
<testcase classname="beryl_scene" name="pose::cheap" time="1.0"/>
<testcase classname="beryl_scene" name="pose::broken" time="0.2"><failure message="test failed"/></testcase>
<testcase classname="beryl_scene" name="pose::long" time="6.0"><failure message="process aborted with signal 15 (SIGTERM)" type="test abort"/></testcase>
</testsuite></testsuites>"""


@pytest.mark.parametrize("broken", [False, True])
def test_the_step_stops_nextest_before_the_hard_limit_and_keeps_what_finished(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    broken: bool,
) -> None:
    """A plan's times are estimates, so the tests can run past the hard limit, which kills the
    step and reports nothing. The step stops nextest `STOP` seconds before the limit instead,
    logs the tests that finished, and defers the rest to the sweep; it fails only for a test
    that failed by itself."""
    logged: list[tuple[str, str, float]] = []
    ran: list[list[str]] = []
    junit: Path = tmp_path / "junit.xml"
    monkeypatch.setattr(test_fit, "JUNIT", junit)
    monkeypatch.setattr(runlog, "result", lambda c, o, s, *_: logged.append((c, o, s)))
    monkeypatch.setattr(test_fit.deadline, "until_hard_limit", lambda: 60.0)
    monkeypatch.setattr(
        test_fit.tokens, "acquire", lambda *_a, **_k: tokens.Lease(4, [], klass="push")
    )
    names = ["cheap", "long", "late", *(["broken"] if broken else [])]
    monkeypatch.setattr(
        rust_fit, "listed", lambda *_a: [f"beryl_scene pose::{n}" for n in names]
    )
    report = (
        STOPPED
        if broken
        else STOPPED.replace('<failure message="test failed"/>', "").replace(
            'name="pose::broken"', 'name="pose::other"'
        )
    )
    nextest = Nextest(ran, junit, JUNIT, 0, seconds=500.0, stopped=report)
    monkeypatch.setattr(test_fit.subprocess, "Popen", nextest)
    records = {f"b3d-test:beryl_scene::pose::{n}": Record(0, 0, 10, 1.0) for n in names}
    # pyre-ignore[6]: a stand-in for test_fit.Known
    assert test_fit.run(Known(records)) == (1 if broken else 0)
    assert nextest.signals == [test_fit.signal.SIGTERM]
    assert ("b3d-test:beryl_scene::pose::cheap", "pass", 1.0) in logged
    assert ("b3d-test:beryl_scene::pose::long", "skip", 0.0) in logged
    assert ("b3d-test:beryl_scene::pose::late", "skip", 0.0) in logged
    if broken:
        assert ("b3d-test:beryl_scene::pose::broken", "fail", 0.2) in logged
    assert "2 tests deferred to the Beryl 3D sweep: stopped" in capsys.readouterr().out
