"""The `beryl3d` lane, which the fast check runs for a change to a Beryl 3D path and
`just b3d lane` runs by hand. Slow CI has no such lane: Beryl 3D's sweep runs after a push
instead (docs/beryl-3d.md, 'Checks and isolation')."""

from __future__ import annotations

import sys

from ..lanes import BY, Lane, Step
from .. import paths

NAME = "beryl3d"
PURITY = [sys.executable, "-m", "beryl_tools.beryl3d.purity"]


def steps() -> list[Step]:
    """The lane's steps, in the order they run. Each check Beryl 3D gains is appended here."""
    cargo = [*BY, "b3d"]
    return [
        Step("fmt", [*cargo, "fmt", "--check"], paths.REPO),
        Step("clippy", [*cargo, "clippy"], paths.REPO),
        Step("cargo-test", [*cargo, "cargo-test"], paths.REPO),
        Step("scene-purity", [*PURITY, "scene-purity"], paths.REPO),
        Step("lints-match", [*PURITY, "lints-match"], paths.REPO),
        Step("data-names", [*PURITY, "data-names"], paths.REPO),
    ]


def lane() -> Lane:
    return Lane(NAME, steps=steps())
