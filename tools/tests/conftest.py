from pathlib import Path

import pytest


@pytest.fixture(autouse=True)
def own_cache(
    tmp_path_factory: pytest.TempPathFactory, monkeypatch: pytest.MonkeyPatch
) -> Path:
    """Every test gets a cache of its own: the run log of a test's `run_lanes` (runlog.py) never
    reaches the machine's, nor a step's results the run that runs pytest."""
    cache = tmp_path_factory.mktemp("cache")
    monkeypatch.setenv("BERYL_CACHE", str(cache))
    monkeypatch.delenv("BERYL_CHECK_RESULTS", raising=False)
    return cache
