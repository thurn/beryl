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
