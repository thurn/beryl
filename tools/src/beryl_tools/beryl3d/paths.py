"""Where Beryl 3D's files are."""

from __future__ import annotations

import hashlib
from pathlib import Path

from .. import paths

# Beryl 3D's Cargo workspace, with its own manifest, lock file and build directory.
WORKSPACE: Path = paths.RUST / "beryl3d"
DATA: Path = paths.REPO / "data" / "beryl3d"
FIXTURE_DEFINITIONS = "data/beryl3d/fixtures.json"


def cache() -> Path:
    """Beryl 3D's part of the per-machine cache, `$BERYL_CACHE/b3d`."""
    path = paths.cache_root() / "b3d"
    path.mkdir(parents=True, exist_ok=True)
    return path


def build_lock() -> Path:
    """The lock under which one build at a time runs in this checkout's Beryl 3D build
    directory (`cargo.build`). It lives in the cache, named for the workspace's path, so that it
    adds no file to the checkout."""
    digest = hashlib.sha256(str(WORKSPACE.resolve()).encode()).hexdigest()[:16]
    return cache() / f"build-{digest}.lock"
