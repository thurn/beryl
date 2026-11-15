"""The efficiency dashboard's paths and its `efficiency` check lane (docs/efficiency-dashboard.md,
"Isolation").

The dashboard is a separate Python project in efficiency/ that imports nothing from the tools
and that nothing imports. A change to its paths selects the `efficiency` lane and, like any
path, `repo`; every other lane is chosen from the rest of the paths, so a change to the
dashboard runs none of the main project's lanes and a change elsewhere never runs its lane.
"""

from __future__ import annotations

import re
import sys
from collections.abc import Iterable
from pathlib import Path

from . import paths, system
from .lanes import Lane

NAME = "efficiency"
# The dashboard's paths, as a change lists them from the repository's root: its project and
# its data files (`data/efficiency-*.json`).
OWN: re.Pattern[str] = re.compile(r"^efficiency/|^data/efficiency-[^/]*\.json$")
PROJECT: Path = paths.REPO / "efficiency"


def split(changed: Iterable[str]) -> tuple[list[str], list[str]]:
    """The `changed` paths that are the dashboard's, and the rest."""
    own: list[str] = []
    rest: list[str] = []
    for path in changed:
        (own if OWN.match(path) else rest).append(path)
    return own, rest


def lane() -> Lane:
    """Formatting, types and unit tests of efficiency/, run with the tools' interpreter, which
    has the same pinned dev tools, so a worktree needs no second environment. It has no
    browser step."""
    made = Lane(NAME).step(
        "black",
        [sys.executable, "-m", "black", "--check", "--quiet", "."],
        PROJECT,
    )
    # Pyre has no Windows build: macOS checks the types.
    if not system.WINDOWS:
        made.step("pyre", ["pyre", "--noninteractive", "check"], PROJECT)
    return made.step("pytest", [sys.executable, "-m", "pytest"], PROJECT)
