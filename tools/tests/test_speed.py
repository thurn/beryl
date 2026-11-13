"""This machine's speed against the reference, and how the checks' clock uses it (speed.py)."""

from __future__ import annotations

import json
import time
from pathlib import Path

import pytest

from beryl_tools import clock, paths, speed


def write(path: Path, seconds: float, workload: int = speed.WORKLOAD) -> None:
    path.write_text(json.dumps({"workload": workload, "seconds": seconds}))


@pytest.fixture
def reference(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    path = tmp_path / "speed-reference.json"
    monkeypatch.setattr(speed, "REFERENCE", path)
    return path


def test_an_unmeasured_machine_counts_as_the_reference(reference: Path) -> None:
    write(reference, 2.0)
    assert speed.factor() == 1.0


def test_no_reference_means_no_adjustment(reference: Path) -> None:
    write(paths.cache_root() / speed.CACHE_FILE, 3.0)
    assert speed.factor() == 1.0


def test_the_factor_is_this_machines_time_over_the_references(reference: Path) -> None:
    write(reference, 2.0)
    write(paths.cache_root() / speed.CACHE_FILE, 3.0)
    assert speed.factor() == 1.5


def test_a_measurement_of_another_workload_is_ignored(reference: Path) -> None:
    write(reference, 2.0)
    write(paths.cache_root() / speed.CACHE_FILE, 3.0, workload=speed.WORKLOAD - 1)
    assert speed.factor() == 1.0


def test_a_broken_measurement_is_held_to_the_limits(reference: Path) -> None:
    write(reference, 1.0)
    write(paths.cache_root() / speed.CACHE_FILE, 100.0)
    assert speed.factor() == speed.LIMITS[1]


def test_a_slower_machine_runs_the_clock_slower(
    reference: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(clock.system, "loadavg", lambda: (0.0, 0.0, 0.0))
    write(reference, 1.0)
    write(paths.cache_root() / speed.CACHE_FILE, 2.0)
    slow = clock.Clock()
    time.sleep(0.2)
    # Half the wall time, give or take scheduling.
    assert 0.08 < slow.now() < 0.15
    assert slow.wall() >= 0.2


def test_the_workload_task_is_deterministic() -> None:
    assert speed.task(7) == speed.task(7)


def test_a_round_counts_its_own_cores_as_load() -> None:
    # Idle before: the workload alone fills the cores, so nothing is adjusted.
    assert speed.adjusted(10.0, 0.0, 10) == 10.0
    # Another 10 runnable processes before: the round shared the cores half and half.
    assert speed.adjusted(10.0, 10.0, 10) == 5.0


def test_the_best_round_is_kept(monkeypatch: pytest.MonkeyPatch) -> None:
    loads = iter([30.0, 0.0])
    monkeypatch.setattr(speed.system, "loadavg", lambda: (next(loads), 0.0, 0.0))
    monkeypatch.setattr(speed, "TASKS", 4)
    seconds, under = speed.measure(rounds=2, gap=0.0)
    assert seconds > 0 and under in (0.0, 30.0)


def test_a_check_starts_one_background_measurement(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    started: list[list[str]] = []
    monkeypatch.delenv(speed.OFF_ENV)
    monkeypatch.setattr(
        speed.subprocess, "Popen", lambda args, **kw: started.append(args)
    )
    assert speed.start_background() is not None
    # A second check run while the first measurement is pending starts none.
    assert speed.start_background() is None
    assert len(started) == 1 and started[0][-2:] == ["speed", "--background"]


def test_a_measured_machine_starts_no_background_measurement(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(speed.OFF_ENV)
    monkeypatch.setattr(speed.subprocess, "Popen", lambda *a, **kw: pytest.fail("ran"))
    write(paths.cache_root() / speed.CACHE_FILE, 2.0)
    assert speed.start_background() is None
