"""Where the site is built and where the beads are read from. Nothing here imports the
repository's tools: the rules are copied from them so that this project stands alone."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

# The page's port unless `--port` names another. The efficiency dashboard uses 8765.
PORT = 8766


def default_cache() -> Path:
    """~/Library/Caches/beryl, or on Windows %LOCALAPPDATA%\\beryl: the same rule as
    tools/src/beryl_tools/paths.py, copied so that nothing here imports the tools."""
    local = os.environ.get("LOCALAPPDATA")
    if sys.platform == "win32" and local:
        return Path(local) / "beryl"
    return Path.home() / "Library" / "Caches" / "beryl"


def cache_root() -> Path:
    root = os.environ.get("BERYL_CACHE")
    return Path(root) if root else default_cache()


def folder() -> Path:
    """The site's part of the cache, `<cache>/status`, made if missing."""
    path = cache_root() / "status"
    path.mkdir(parents=True, exist_ok=True)
    return path


def site() -> Path:
    """The built site: what the server serves and what `publish` pushes."""
    return folder() / "site"


def changelogs() -> Path:
    """Each bead's events from `bd history --events`, one JSON file per bead. It is not made
    here, because every page's build asks for it; `changelog.store` makes it."""
    return cache_root() / "status" / "changelog"


def main_checkout(start: Path | None = None) -> Path:
    """The main checkout, whose `.beads` holds the database: the parent of the repository's
    common git folder, so that a worktree finds it too. `bd` is run there."""
    here = start or Path(__file__).resolve().parent
    common = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=here,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    return Path(common).parent


def store_files(main: Path) -> list[Path]:
    """The files whose modification times change when the beads database does: `bd` touches
    `last-touched` on a write, and Dolt rewrites its manifest and journal index on any commit,
    including one a sync pulled in."""
    noms = main / ".beads" / "embeddeddolt" / "by" / ".dolt" / "noms"
    return [
        main / ".beads" / "last-touched",
        noms / "manifest",
        noms / "journal.idx",
    ]
