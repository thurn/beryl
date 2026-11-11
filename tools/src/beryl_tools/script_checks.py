"""The script lane's step (checks.py): `beryl_cli script-check` in a debug build, where every
script the session builds is held to the replay check and the rule invariants (plan K24). It
plays every card's every spec moment from both seats and records every UI fixture, and reports
each to the run log itself as `script:<card>/<moment>/<seat>` or
`script:fixtures/<fixture>/<seat>` (runlog.py), so a panic fails the one check it happened in.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import IO, TYPE_CHECKING

from . import paths, system
from .reruns import CANNOT_RUN, failed_scripts

if TYPE_CHECKING:
    from .checks import Lane

FIXTURES: Path = paths.UI / "fixtures.json"
# The crate that runs the checks; it and the crates it depends on decide every script.
CHECKER = "beryl_cli"


def lane() -> Lane:
    """Every card's spec moments from both seats and every UI fixture's recording, in a debug
    build that holds each script to the replay check and the rule invariants (plan K24): one
    check per moment, seat and recording."""
    from .checks import Lane  # It imports this module.

    cmd = [sys.executable, "-m", "beryl_tools.script_checks"]
    return Lane("script", warm=True).step("script-check", cmd, rerun=failed_scripts)


def script_check_step(*only: str) -> int:
    """Every script check, or `only` those named (a check's id, or what ids begin with, such as
    a card's slug)."""
    if not build():
        return CANNOT_RUN
    return run(list(only))


def build() -> bool:
    """Builds the checker in debug, where the script checks are assertions; whether it built."""
    built = subprocess.run(
        ["cargo", "build", "--locked", "--quiet", "--package", CHECKER],
        cwd=paths.RUST,
        env=paths.run_env(),
        stdin=subprocess.DEVNULL,
        check=False,
    )
    return built.returncode == 0


def run(
    only: list[str], results: Path | None = None, out: IO[str] | None = None
) -> int:
    """Runs the built checker on every check or `only` those named, logging each check to
    `results` when given (`$BERYL_CHECK_RESULTS` otherwise) and printing to `out`."""
    env = dict(os.environ)
    if results is not None:
        env["BERYL_CHECK_RESULTS"] = str(results)
    binary = system.exe(paths.RUN_TARGET / "debug" / CHECKER)
    return subprocess.run(
        [str(binary), "script-check", str(FIXTURES), *only],
        cwd=paths.RUST,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=out,
        check=False,
    ).returncode


def inputs(crates: Path = paths.RUST / "crates") -> str:
    """A pattern of the paths a change to which can change a script check's outcome: the
    workspace's manifest and lock, the fixtures' definitions, and the checker's crate with
    every crate it depends on through path dependencies."""
    found: set[str] = set()
    todo = [CHECKER]
    while todo:
        crate = todo.pop()
        if crate in found:
            continue
        found.add(crate)
        manifest = tomllib.loads((crates / crate / "Cargo.toml").read_text())
        for table in ("dependencies", "build-dependencies"):
            for spec in manifest.get(table, {}).values():
                if isinstance(spec, dict) and isinstance(spec.get("path"), str):
                    todo.append(Path(spec["path"]).name)
    names = "|".join(re.escape(c) for c in sorted(found))
    return rf"^rust/(Cargo\.(toml|lock)$|crates/({names})/)|^ui/fixtures\.json$"


if __name__ == "__main__":
    sys.exit(script_check_step(*sys.argv[1:]))
