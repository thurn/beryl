"""Saves the sweep's build directory for `just b3d` to clone (beryl3d/sweep.py, 'Cache')."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from .. import caches, system, warm
from .. import paths as repo_paths
from . import paths


def save_target(target: Path, cached: Path) -> bool:
    """Replaces `cached` with a clone of the build directory `target`, less its incremental
    caches: rustc keys each to the absolute path of the tree that built it and discards it
    anywhere else (caches.save_kept). Under the lock `just b3d` clones the cache with.

    The clone is held to the main cache's budget (warm.py, `prune`) before it replaces the
    cache. Without that, every saved cache kept every artifact of every earlier build, and every
    new worktree cloned them all: on 2026-11-16 the cache was 44 GB, with 77,077 files in
    debug/deps and nine copies of `libberyl_scene-*.rlib`, and free disk fell 174 GB in two days.
    """
    if not target.is_dir():
        return False
    copy = cached.with_name(f".{cached.name}-new-{os.getpid()}")
    old = cached.with_name(f".{cached.name}-old-{os.getpid()}")
    with system.file_lock(paths.target_lock()):
        shutil.rmtree(copy, ignore_errors=True)
        if not warm.clone_into(target, copy):
            return False
        for incremental in caches.incremental_folders(copy):
            shutil.rmtree(incremental, ignore_errors=True)
        warm.prune(copy)
        # The sources' times as this build saw them, which `cli.warm_target` gives the files
        # of a new worktree after it clones the cache.
        warm.record_sources(repo_paths.REPO, copy)
        if cached.exists():
            cached.rename(old)
        copy.rename(cached)
    shutil.rmtree(old, ignore_errors=True)
    return True
