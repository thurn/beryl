"""Where the site is built and where the beads are read from. Nothing here imports the
repository's tools: the rules are copied from them so that this project stands alone."""

from __future__ import annotations

import functools
import hashlib
import os
import shutil
import subprocess
import sys
import time
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


# Site folders unused for this long are deleted by `prune_sites`.
SITE_DAYS = 2


@functools.cache
def code_version() -> str:
    """A digest of this project's code (`beryl_status` and `cirrus`, Python and static files)
    as it was when this process first asked. Each version of the code builds into its own
    site folder, so two builders running different code, such as a worktree's server and the
    agent's publish from the main checkout, never overwrite each other's pages."""
    return code_digest()


def code_digest() -> str:
    """The digest `code_version` names, read from the files now: the server compares the
    two to notice that its code changed on disk."""
    digest = hashlib.sha256()
    source = Path(__file__).resolve().parent.parent
    for package in ("beryl_status", "cirrus"):
        for path in sorted((source / package).rglob("*")):
            if path.is_file() and path.suffix in (".py", ".css", ".js"):
                digest.update(path.relative_to(source).as_posix().encode())
                digest.update(path.read_bytes())
    return digest.hexdigest()[:12]


def sites() -> Path:
    return folder() / "sites"


def site() -> Path:
    """The built site of this version of the code: what the server serves and what
    `publish` pushes. Builders running the same code share it."""
    return sites() / code_version()


def last_site(exclude: Path) -> Path | None:
    """The most recently built site of another code version, or None. A server whose code
    just changed answers from it, in place, until its own first build finishes: copying its
    7,000 files took up to 30 seconds on a busy disk."""
    candidates = [folder() / "site"]
    if sites().is_dir():
        candidates += [p for p in sites().iterdir() if p != exclude]
    built = [p for p in candidates if (p / "index.html").is_file()]
    return max(built, key=lambda p: p.stat().st_mtime) if built else None


def prune_sites(keep: Path) -> list[Path]:
    """Deletes the site folders of other code versions unused for `SITE_DAYS`, and the
    single `site` folder older versions built; returns what it deleted. A build touches its
    own folder, so a folder in use is never old."""
    removed: list[Path] = []
    cutoff = time.time() - SITE_DAYS * 86400
    candidates = [folder() / "site"]
    if sites().is_dir():
        candidates += [p for p in sites().iterdir() if p.is_dir()]
    for path in candidates:
        if path == keep or not path.is_dir():
            continue
        if path.name == "site" or path.stat().st_mtime < cutoff:
            shutil.rmtree(path, ignore_errors=True)
            removed.append(path)
    return removed


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
