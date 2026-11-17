"""The standalone Python projects' paths and check lanes, including the efficiency dashboard.

Each project is a separate Python package that imports nothing from the tools and that nothing
imports. A change to a project's paths selects that project's lane and, like any path, `repo`;
every other lane is chosen from the rest of the paths, so a change to a project runs none of
the main project's lanes and a change elsewhere never runs its lane.
"""

from __future__ import annotations

import re
import sys
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from . import paths, system
from .lanes import Lane


@dataclass(frozen=True)
class Project:
    """A standalone project: its lane's name, which is also its folder, and the paths, as a
    change lists them from the repository's root, that are its own."""

    name: str
    own: re.Pattern[str]

    @property
    def folder(self) -> Path:
        return paths.REPO / self.name

    def lane(self) -> Lane:
        """Formatting, types and unit tests of the project, run with the tools' interpreter,
        which has the same pinned dev tools, so a worktree needs no second environment. It has
        no browser step."""
        made = Lane(self.name).step(
            "black",
            [sys.executable, "-m", "black", "--check", "--quiet", "."],
            self.folder,
        )
        # Pyre has no Windows build: macOS checks the types.
        if not system.WINDOWS:
            made.step("pyre", ["pyre", "--noninteractive", "check"], self.folder)
        return made.step("pytest", [sys.executable, "-m", "pytest"], self.folder)


EFFICIENCY = Project(
    "efficiency", re.compile(r"^efficiency/|^data/efficiency-[^/]*\.json$")
)
PROJECTS: tuple[Project, ...] = (EFFICIENCY,)
NAMES: frozenset[str] = frozenset(p.name for p in PROJECTS)


def split(changed: Iterable[str]) -> tuple[dict[str, list[str]], list[str]]:
    """The `changed` paths of each standalone project, by the project's name (a project with
    none is left out), and the rest."""
    own: dict[str, list[str]] = {}
    rest: list[str] = []
    for path in changed:
        found = next((p for p in PROJECTS if p.own.match(path)), None)
        if found is None:
            rest.append(path)
        else:
            own.setdefault(found.name, []).append(path)
    return own, rest


def project(name: str) -> Project:
    return next(p for p in PROJECTS if p.name == name)
