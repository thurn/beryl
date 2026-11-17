"""Tests for step_runner.py: the CPU time a killed step is charged."""

from __future__ import annotations

import sys
import threading

import pytest

from beryl_tools import paths, step_runner, system

# A step whose child spins until it is killed: the shape of a `record` step whose cargo build
# is still compiling when the run's limit kills it.
SPINNING_CHILD = (
    "import subprocess, sys; "
    "subprocess.run([sys.executable, '-c', 'while True: pass'])"
)


def test_cpu_time_reads_every_ps_format() -> None:
    assert system.cpu_time("0:01.50") == pytest.approx(1.5)
    assert system.cpu_time("7298:32.56") == pytest.approx(7298 * 60 + 32.56)
    assert system.cpu_time("1:02:03") == pytest.approx(3723)
    assert system.cpu_time("2-01:00:00") == pytest.approx(2 * 86_400 + 3600)


@pytest.mark.skipif(system.WINDOWS, reason="Windows records no CPU time for a step")
def test_a_killed_step_is_charged_the_cpu_its_children_used() -> None:
    steps = step_runner.Steps()
    code, _ = steps.run(
        [sys.executable, "-c", SPINNING_CHILD], paths.REPO, {}, cores=0, limit=1.5
    )
    assert code is None
    # The child spun for most of the 1.5 s; reaping the killed step alone gave about 0.03 s.
    assert steps.cpu[threading.get_ident()] > 0.5
