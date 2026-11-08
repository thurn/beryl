"""Where things are: this checkout, its parts, the per-machine cache, the agent."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

# tools/src/beryl_tools/paths.py: the checkout (a worktree or the main checkout) is four up.
REPO: Path = Path(__file__).resolve().parents[3]
RUST: Path = REPO / "rust"
UI: Path = REPO / "ui"
APP: Path = RUST / "crates" / "beryl_app"
TOOLS: Path = REPO / "tools"

# Environment variables that mark a run as an agent's: it must stay windowless and silent.
AGENT_ENV = (
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CODEX_THREAD_ID",
    "ENACT_SESSION",
    "CI",
)


def default_cache() -> Path:
    """~/Library/Caches/beryl, or on Windows %LOCALAPPDATA%\beryl."""
    local = os.environ.get("LOCALAPPDATA")
    if sys.platform == "win32" and local:
        return Path(local) / "beryl"
    return Path.home() / "Library" / "Caches" / "beryl"


def cache_root() -> Path:
    """The per-machine cache shared by every checkout: $BERYL_CACHE, else the default."""
    root = os.environ.get("BERYL_CACHE")
    path = Path(root) if root else default_cache()
    path.mkdir(parents=True, exist_ok=True)
    return path


def is_agent() -> bool:
    return any(os.environ.get(name) for name in AGENT_ENV)


def main_checkout() -> Path | None:
    """The main checkout this worktree belongs to (itself, outside a worktree)."""
    run = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=False,
    )
    return Path(run.stdout.strip()).parent if run.returncode == 0 else None


# The Tauri CLI's builds get their own target directory: it sets environment variables that
# Tauri's build scripts track, so sharing one with plain cargo would rebuild Tauri every time
# the two alternate, and parallel lanes would wait on each other's build lock.
TAURI_TARGET: Path = RUST / "target" / "tauri"


# The target directory of the binaries `by` runs while other work builds: on Windows, one of
# their own. Windows replaces no running binary, and `cargo test --workspace` (dev-dependencies'
# features unified in) and `cargo build -p beryl_cli` build different binaries at one path, so
# the rust lane would fail to relink the binary a UI lane runs, and the other way round.
RUN_TARGET: Path = (
    RUST / "target" / "run" if sys.platform == "win32" else RUST / "target"
)


def run_env() -> dict[str, str]:
    """The environment for cargo building a binary `by` runs ([`RUN_TARGET`])."""
    return {**os.environ, "CARGO_TARGET_DIR": str(RUN_TARGET)}


def tauri_env() -> dict[str, str]:
    """The environment for the Tauri CLI, run from ui/: it finds the app crate through
    TAURI_APP_PATH."""
    return {
        **os.environ,
        "TAURI_APP_PATH": str(APP),
        "CARGO_TARGET_DIR": str(TAURI_TARGET),
    }
