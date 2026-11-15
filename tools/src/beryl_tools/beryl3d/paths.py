"""Where Beryl 3D's files are, and how a list of changed paths splits into Beryl 3D's and the
rest of the project's."""

from __future__ import annotations

import re
from collections.abc import Iterable
from pathlib import Path

from .. import paths

# Beryl 3D's paths, as a change lists them from the repository's root. A change to any of
# them selects the `beryl3d` lane and no other but `repo` and `names`. Its tools, under
# tools/src/beryl_tools/beryl3d, are not among them: a change to the tools runs the python
# lane, as any change to tools/ does.
OWN: re.Pattern[str] = re.compile(r"^(rust/beryl3d|data/beryl3d)/")
# Beryl 3D's Cargo workspace, with its own manifest, lock file and build directory.
WORKSPACE: Path = paths.RUST / "beryl3d"
DATA: Path = paths.REPO / "data" / "beryl3d"
# The epic under which Beryl 3D's sweep files the failures it finds.
EPIC = "by-q44s"


def cache() -> Path:
    """Beryl 3D's part of the per-machine cache, `$BERYL_CACHE/b3d`."""
    path = paths.cache_root() / "b3d"
    path.mkdir(parents=True, exist_ok=True)
    return path


def target_lock() -> Path:
    """The lock under which the cache's build directory is cloned into a worktree or replaced
    by the sweep, so a clone never reads a directory the sweep is swapping out."""
    return cache() / ".target.lock"


def split(changed: Iterable[str]) -> tuple[list[str], list[str]]:
    """The `changed` paths that are Beryl 3D's, and the rest."""
    own: list[str] = []
    rest: list[str] = []
    for path in changed:
        (own if OWN.match(path) else rest).append(path)
    return own, rest
