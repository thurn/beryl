import json
import os
from pathlib import Path

import pytest

from beryl_tools import caches

# Where a check wants each test's result, and the lane it belongs to (python_tests.py), read once
# at import: `own_cache` below removes the variable while each test runs.
RESULTS: str | None = os.environ.get("BERYL_CHECK_RESULTS")
LANE: str = os.environ.get("BERYL_PYTEST_LANE", "python")


@pytest.fixture(autouse=True)
def own_cache(
    tmp_path_factory: pytest.TempPathFactory, monkeypatch: pytest.MonkeyPatch
) -> Path:
    """Every test gets a cache of its own: the run log of a test's `run_lanes` (runlog.py) never
    reaches the machine's, nor a step's results the run that runs pytest. Nor do its verdicts
    reach the beads (verdicts.py). Its volume has room, whatever the machine's disk holds: on a
    nearly full one, low-disk mode (caches.py) pruned what tests had cached (by-ix14). A test of
    low-disk mode sets the free space it wants."""
    cache = tmp_path_factory.mktemp("cache")
    monkeypatch.setenv("BERYL_CACHE", str(cache))
    monkeypatch.setenv("BERYL_CI_BEADS", "off")
    monkeypatch.setenv("BERYL_SPEED", "off")
    monkeypatch.delenv("BERYL_CHECK_RESULTS", raising=False)
    monkeypatch.setattr(caches, "free_space", lambda _: 10 * caches.LOW_DISK)
    return cache


def pytest_runtest_logreport(report: pytest.TestReport) -> None:
    """Under a check (`$BERYL_CHECK_RESULTS`, runlog.py), reports each test's outcome as a check
    of the lane python_tests.py names, so a failure or flake names its test. Only the main
    process writes: pytest-xdist passes each worker's reports to it."""
    results = RESULTS
    if not results or os.environ.get("PYTEST_XDIST_WORKER"):
        return
    if report.when != "call" and not (report.failed or report.skipped):
        return
    outcome = "fail" if report.failed else "skip" if report.skipped else "pass"
    line = {
        "check": f"{LANE}:{report.nodeid}",
        "outcome": outcome,
        "seconds": round(report.duration, 3),
    }
    with open(results, "a") as out:
        out.write(json.dumps(line) + "\n")
