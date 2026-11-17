"""The fast check's time contract (deadline.py) and the steps that plan against it."""

import time
from pathlib import Path

import pytest

from beryl_tools import checks, deadline, oracle_selection, runlog, rust_fit
from beryl_tools.selection import Record


def test_the_target_stretches_with_load_up_to_ninety_seconds() -> None:
    assert deadline.target_wall(load=4, cpus=16, factor=1.0) == 60
    assert deadline.target_wall(load=24, cpus=16, factor=1.0) == 90
    assert deadline.target_wall(load=4, cpus=16, factor=1.2) == 72


def test_left_counts_from_the_start_the_fast_check_set(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(deadline.STARTED_ENV, raising=False)
    assert deadline.left() is None
    monkeypatch.setattr(deadline.system, "loadavg", lambda: (0.0, 0.0, 0.0))
    monkeypatch.setattr(deadline.speed, "factor", lambda: 1.0)
    monkeypatch.setenv(deadline.STARTED_ENV, "1000")
    assert deadline.left(now=1020) == 40


def test_a_step_window_ends_at_the_target_but_gives_a_late_step_its_floor(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A Beryl 3D test step that starts after the target gets `STEP_FLOOR` seconds, not every
    second up to the hard limit, and the window's end stays where its first call put it.
    """
    monkeypatch.setattr(deadline.system, "loadavg", lambda: (0.0, 0.0, 0.0))
    monkeypatch.setattr(deadline.speed, "factor", lambda: 1.0)
    monkeypatch.setenv(deadline.STARTED_ENV, "1000")
    # Started 10 s in: the target, 50 s away, is past the floor.
    monkeypatch.setattr(deadline, "_STEP_END", [])
    assert deadline.step_limit(now=1010) == 50
    # Started 100 s in, past the target: the floor.
    monkeypatch.setattr(deadline, "_STEP_END", [])
    assert deadline.step_limit(now=1100) == deadline.STEP_FLOOR
    assert deadline.step_limit(now=1110) == deadline.STEP_FLOOR - 10
    # Started 160 s in: never past the hard limit.
    monkeypatch.setattr(deadline, "_STEP_END", [])
    assert deadline.step_limit(now=1160) == deadline.HARD_LIMIT - 160
    monkeypatch.delenv(deadline.STARTED_ENV)
    assert deadline.step_limit() is None


def test_a_step_cut_off_at_the_hard_limit_is_deferred_and_fails_nothing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    lane = (
        checks.Lane("slow")
        .step("sleep", ["sleep", "30"], tmp_path)
        .step("after", ["true"], tmp_path)
    )
    started = time.monotonic()
    report = checks.run_all([lane], "check", 60, wall_limit=1.0)
    assert time.monotonic() - started < 20
    assert report.code == 0
    out = capsys.readouterr().out
    assert "deferred to slow CI at the 180 s hard limit: sleep, after" in out
    assert "deferred: slow sleep" in out


def test_slow_ci_still_fails_a_step_its_wall_limit_cuts_off(tmp_path: Path) -> None:
    lane = checks.Lane("slow").step("sleep", ["sleep", "30"], tmp_path)
    assert checks.run_all([lane], "ci", 60, wall_limit=1.0).code == 1


def test_untimed_tests_run_first_then_catches_then_the_cheapest() -> None:
    known = {
        "rust:a::cheap": Record(0, 0, 10, 1.0),
        "rust:a::caught": Record(2, 2, 10, 20.0),
        "rust:a::slow": Record(0, 0, 10, 100.0),
        "rust:doctests": Record(0, 0, 10, 50.0),
    }
    tests = ["a cheap", "a caught", "a slow", "a new"]
    chosen, beside, cut = rust_fit.plan(tests, ["rust:doctests"], known, 35.0, 1)
    # The new test is planned at the default 3 s: 3 + 20 + 1 fits 30 s, 50 and 100 do not.
    assert chosen == ["a cheap", "a caught", "a new"]
    assert beside == []
    assert cut == ["rust:doctests", "rust:a::slow"]


def test_the_oracle_lane_plays_the_cards_that_fit_cheapest_first(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    skipped: list[tuple[str, str, float]] = []
    monkeypatch.setattr(runlog, "result", lambda c, o, s, *_: skipped.append((c, o, s)))
    monkeypatch.setattr(deadline, "left", lambda: 30.0)
    entries: list[dict[str, object]] = [
        {"card": "Big", "name": "s1"},
        {"card": "Big", "name": "s2"},
        {"card": "Small", "name": "s1"},
    ]
    known = {
        "oracle:Big/s1/xmage": Record(0, 0, 1, 10.0),
        "oracle:Big/s2/xmage": Record(0, 0, 1, 10.0),
        "oracle:Small/s1/xmage": Record(0, 0, 1, 5.0),
    }
    # Small: 5 + 8 (Forge, untimed) fits 30 s; Big: 10 + 10 + 8 + 8 does not as well.
    kept = oracle_selection.fitted(entries, known)
    assert kept == [{"card": "Small", "name": "s1"}]
    assert ("oracle:Big/s1/forge", "skip", 0.0) in skipped


def test_the_oracle_lane_fits_generated_scenarios_one_at_a_time(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(runlog, "result", lambda *_: None)
    monkeypatch.setattr(deadline, "left", lambda: 40.0)
    # The generated scenarios are unrelated positions under one card: each counts alone, so
    # two of three fit (8 + 8 s each) where the card as a whole would not.
    entries: list[dict[str, object]] = []
    for i in range(3):
        entries.append({"card": "Generated", "name": f"Generated {i}"})
    kept = oracle_selection.fitted(entries, {})
    assert [e["name"] for e in kept] == ["Generated 0", "Generated 1"]
