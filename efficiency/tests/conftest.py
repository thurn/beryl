"""Shared fixtures: a store in a temporary cache, and small synthetic sources. The repository
is public, so no test reads a real transcript, bead or command line from a machine."""

from __future__ import annotations

from pathlib import Path

import pytest


@pytest.fixture(autouse=True)
def cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Every test's `$BERYL_CACHE`, so none touches the machine's store."""
    path = tmp_path / "cache"
    monkeypatch.setenv("BERYL_CACHE", str(path))
    return path
