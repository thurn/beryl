"""Shared fixtures: a store in a temporary cache, and small synthetic sources. The repository
is public, so no test reads a real transcript, ticket or command line from a machine."""

from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path

import pytest

from beryl_efficiency import scope


@pytest.fixture
def utc_days(monkeypatch: pytest.MonkeyPatch) -> None:
    """Use UTC for the reports' calendar buckets on every host."""

    def as_utc(moment: datetime) -> datetime:
        if moment.tzinfo is None:
            return moment.replace(tzinfo=timezone.utc)
        return moment.astimezone(timezone.utc)

    monkeypatch.setattr(scope, "local_time", as_utc)


@pytest.fixture(autouse=True)
def cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Every test's `$BERYL_CACHE`, so none touches the machine's store."""
    path = tmp_path / "cache"
    monkeypatch.setenv("BERYL_CACHE", str(path))
    return path
