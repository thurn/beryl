"""Each code version builds into its own site folder; a new folder starts from the newest
build, and old folders are deleted."""

from __future__ import annotations

import os
import time
from pathlib import Path

from beryl_status import agent, paths


def built(folder: Path, text: str, age_days: float = 0) -> Path:
    folder.mkdir(parents=True)
    (folder / "index.html").write_text(text)
    when = time.time() - age_days * 86400
    os.utime(folder, (when, when))
    return folder


def test_the_site_folder_is_named_by_the_code_version() -> None:
    assert paths.site() == paths.folder() / "sites" / paths.code_version()
    assert len(paths.code_version()) == 12


def test_a_new_folder_is_seeded_from_the_newest_build() -> None:
    built(paths.sites() / "older", "old", age_days=1)
    built(paths.sites() / "newer", "new")
    target = paths.sites() / "mine"
    assert paths.seed_site(target) == paths.sites() / "newer"
    assert (target / "index.html").read_text() == "new"


def test_nothing_to_seed_from() -> None:
    assert paths.seed_site(paths.sites() / "mine") is None


def test_prune_keeps_recent_folders_and_its_own() -> None:
    mine = built(paths.sites() / "mine", "x", age_days=5)
    recent = built(paths.sites() / "recent", "x", age_days=1)
    stale = built(paths.sites() / "stale", "x", age_days=3)
    legacy = built(paths.folder() / "site", "x")
    assert sorted(paths.prune_sites(mine)) == sorted([stale, legacy])
    assert mine.is_dir() and recent.is_dir()


def test_the_agent_watches_the_beads_and_is_throttled(tmp_path: Path) -> None:
    plist = agent.agent_plist(tmp_path, "/usr/bin/uv")
    watched = plist["WatchPaths"]
    assert isinstance(watched, list)
    assert f"{tmp_path.as_posix()}/.beads/last-touched" in watched
    assert plist["ThrottleInterval"] == 600
    assert plist["StartInterval"] == 3600
