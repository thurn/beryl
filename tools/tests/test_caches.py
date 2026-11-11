import os
import time
from datetime import timedelta
from pathlib import Path

import pytest

from beryl_tools import caches

NOW: float = time.time()
DAY: float = timedelta(days=1).total_seconds()


def folder(cache: Path, kind: str, name: str, days_old: float) -> Path:
    """A checkout's folder of `kind` holding one screenshot, all last changed `days_old` ago."""
    path = cache / kind / name
    path.mkdir(parents=True)
    shot = path / "shot.png"
    shot.write_bytes(b"x" * 10_000)
    stamp = NOW - days_old * DAY
    for entry in (shot, path):
        os.utime(entry, (stamp, stamp))
    return path


def test_gone_checkouts_old_folders_go_and_live_ones_stay(tmp_path: Path) -> None:
    gone = folder(tmp_path, "ui-shots", "ci-0123abcd", days_old=2)
    gone_test = folder(tmp_path, "ui-test", "ci-0123abcd", days_old=3)
    live = folder(tmp_path, "ui-shots", "beryl", days_old=30)
    removed = caches.prune(tmp_path, {"beryl", "by-x.1"}, NOW)
    assert sorted(r.path for r in removed) == sorted([gone, gone_test])
    assert all(r.size >= 10_000 for r in removed)
    assert not gone.exists() and not gone_test.exists()
    assert (live / "shot.png").exists()


def test_a_gone_checkouts_folder_changed_within_a_day_stays(tmp_path: Path) -> None:
    fresh = folder(tmp_path, "spec-film", "ci-new", days_old=0.5)
    stale = folder(tmp_path, "spec-film", "ci-old", days_old=2)
    # A run just wrote into the old folder: it is in use, whatever git says.
    (stale / "new.png").write_bytes(b"y")
    assert caches.prune(tmp_path, {"beryl"}, NOW) == []
    assert fresh.exists() and stale.exists()


def test_other_cache_folders_are_left_alone(tmp_path: Path) -> None:
    oracle = folder(tmp_path, "oracles", "ci-gone", days_old=10)
    assert caches.prune(tmp_path, {"beryl"}, NOW) == []
    assert oracle.exists()


def test_a_dry_run_removes_nothing(tmp_path: Path) -> None:
    gone = folder(tmp_path, "ui-test", "agent-1", days_old=5)
    removed = caches.prune(tmp_path, {"beryl"}, NOW, dry_run=True)
    assert [r.path for r in removed] == [gone]
    assert gone.exists()


def test_no_live_checkouts_refuses_rather_than_removing_everything(
    tmp_path: Path,
) -> None:
    live = folder(tmp_path, "ui-shots", "beryl", days_old=5)
    with pytest.raises(ValueError):
        caches.prune(tmp_path, set(), NOW)
    assert live.exists()
