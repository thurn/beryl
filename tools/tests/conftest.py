from pathlib import Path

import pytest

from beryl_tools import caches


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
    monkeypatch.delenv("BERYL_CHECK_RESULTS", raising=False)
    monkeypatch.setattr(caches, "free_space", lambda _: 10 * caches.LOW_DISK)
    return cache
