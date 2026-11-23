"""Where things are: this checkout, its parts, the per-machine cache, the agent."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

# tools/src/beryl_tools/paths.py: the checkout (a worktree or the main checkout) is four up.
REPO: Path = Path(__file__).resolve().parents[3]

# `by`, run by a `by` command as a child with the same interpreter.
BY: list[str] = [sys.executable, "-m", "beryl_tools.cli"]
RUST: Path = REPO / "rust"
TOOLS: Path = REPO / "tools"

# Environment variables that mark a run as an agent's: it must stay windowless and silent.
AGENT_ENV = (
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CODEX_THREAD_ID",
    "IGNITE_SESSION",
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


def main_checkout(checkout: Path | None = None) -> Path | None:
    """The main checkout this worktree belongs to (itself, outside a worktree)."""
    run = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=checkout or REPO,
        capture_output=True,
        text=True,
        check=False,
    )
    if run.returncode:
        return None
    common = Path(run.stdout.strip())
    return common.parent if common.name == ".git" else common


def worktree_paths() -> list[Path]:
    """The paths of this repository's worktrees, the main checkout's included; none if git
    could not list them."""
    listing = subprocess.run(
        ["git", "-C", str(REPO), "worktree", "list", "--porcelain"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout
    return [
        Path(line.removeprefix("worktree "))
        for line in listing.splitlines()
        if line.startswith("worktree ")
    ]


def worktree_names() -> list[str]:
    """The names of this repository's worktrees, the main checkout's included; none if git
    could not list them."""
    return [path.name for path in worktree_paths()]


# Clippy gets its own target directory too. Cargo holds one lock on a target directory for the
# whole of a command, and clippy's units (check metadata) are not the ones `cargo build` and
# `cargo test` produce, so the two share no work but did wait on each other: in the old fast
# check, clippy printed "Blocking waiting for file lock on build directory" while the browser,
# script and oracle steps built `beryl_cli` ([`CLI_BUILD`]), and from 2026-11-14 to 16 it was killed at
# the 180 s hard limit in 102 of 160 runs. In its own directory it runs beside the build.
CLIPPY_TARGET: Path = RUST / "target" / "clippy"


# The target directory of the binaries `by` runs while other work builds: on Windows, one of
# their own. Windows replaces no running binary, so a Rust test build would fail to relink
# a binary a browser check is running, and the other way round.
RUN_TARGET: Path = (
    RUST / "target" / "run" if sys.platform == "win32" else RUST / "target"
)


# The package every Rust build leaves out: a cold build of its iroh dependency is slow, so it
# has a check family of its own (`iroh` in ignite/checks/rest.toml).
NIGHTLY_PACKAGE = "beryl_link_iroh"

# What the Rust test and CLI builds name: the whole workspace but [`NIGHTLY_PACKAGE`].
BUILD_SCOPE: list[str] = ["--workspace", "--exclude", NIGHTLY_PACKAGE]

# The command that builds the binaries the checks run: `beryl_cli` for the script checks, the
# fixture recordings of the browser checks and the spec pages, and `beryl_scenario` for the
# oracle checks' scenario dump. It must build the same units as the tests
# step, which shares the target directory with it. `cargo test` builds every library in the
# `test` profile, and Cargo's hash for a unit also depends on which packages the command names,
# so `cargo build -p beryl_cli` compiled the rules, the engine, every card crate and the CLI a
# second time, in the `dev` profile, while the other steps waited on Cargo's lock on the target
# directory. In the old fast check on 2026-11-14 and 15, 69 of 176 rust:clippy runs were killed at
# the 180 s limit while waiting on that lock. Built over [`BUILD_SCOPE`] in the `test` profile,
# whichever of the two runs second compiles nothing the first did: after edits to beryl_kernel
# and beryl_table and the tests step's build, this command only linked the CLI, in 2 s. The
# binaries land in target/debug. tools/tests/test_cli_build.py keeps the features the same.
CLI_BUILD: list[str] = [
    "cargo",
    "build",
    "--locked",
    "--quiet",
    "--profile",
    "test",
    *BUILD_SCOPE,
    # With the libraries named too, Cargo hashes them as `cargo test` does; with `--bin` alone,
    # beryl_kernel, beryl_table, beryl_rules and beryl_engine still compiled a second time.
    "--lib",
    "--bin",
    "beryl_cli",
    "--bin",
    "beryl_scenario",
]


def run_env() -> dict[str, str]:
    """The environment for cargo building a binary `by` runs ([`RUN_TARGET`])."""
    return {**os.environ, "CARGO_TARGET_DIR": str(RUN_TARGET)}


def clippy_env() -> dict[str, str]:
    """The environment for cargo clippy ([`CLIPPY_TARGET`])."""
    return {**os.environ, "CARGO_TARGET_DIR": str(CLIPPY_TARGET)}
