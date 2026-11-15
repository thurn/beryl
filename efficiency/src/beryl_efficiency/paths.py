"""Where the store lives and what machine this is. Nothing here imports the repository's tools:
the rules are copied from them so that this project stands alone."""

from __future__ import annotations

import os
import platform
import sys
from pathlib import Path


def default_cache() -> Path:
    """~/Library/Caches/beryl, or on Windows %LOCALAPPDATA%\\beryl: the same rule as
    tools/src/beryl_tools/paths.py, copied so that nothing here imports the tools."""
    local = os.environ.get("LOCALAPPDATA")
    if sys.platform == "win32" and local:
        return Path(local) / "beryl"
    return Path.home() / "Library" / "Caches" / "beryl"


def cache_root() -> Path:
    """The per-machine cache shared by every checkout: `$BERYL_CACHE`, else the default."""
    root = os.environ.get("BERYL_CACHE")
    return Path(root) if root else default_cache()


def folder() -> Path:
    """The dashboard's part of the cache, `<cache>/efficiency`, made if missing."""
    path = cache_root() / "efficiency"
    path.mkdir(parents=True, exist_ok=True)
    return path


def store() -> Path:
    return folder() / "store.sqlite"


def lock() -> Path:
    """The file whose lock lets one ingest run at a time."""
    return folder() / "ingest.lock"


def host() -> str:
    """This machine's name as every row records it: the node name without a domain, in lower
    case."""
    return platform.node().split(".")[0].lower() or "unknown"


def claude_projects() -> Path:
    """Where Claude Code keeps its transcripts, one folder per working directory."""
    return Path.home() / ".claude" / "projects"


def claude_sessions() -> Path:
    """Where the Claude desktop app keeps its session records."""
    if sys.platform == "win32":
        base = Path(os.environ.get("APPDATA") or Path.home() / "AppData" / "Roaming")
    else:
        base = Path.home() / "Library" / "Application Support"
    return base / "Claude" / "claude-code-sessions"


def codex_sessions() -> list[Path]:
    """Where Codex keeps its logs: `sessions/<year>/<month>/<day>/rollout-*.jsonl`, and the
    flat `archived_sessions/` that finished ones are moved to."""
    home = Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")
    return [home / "sessions", home / "archived_sessions"]
