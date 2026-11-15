"""The `beryl3d` lane, which the fast check runs for a change to a Beryl 3D path and
`just b3d lane` runs by hand. Slow CI has no such lane: Beryl 3D's sweep runs after a push
instead (docs/beryl-3d.md, 'Checks and isolation')."""

from __future__ import annotations

import sys

from ..lanes import BY, Lane, Step
from .. import paths

NAME = "beryl3d"
PURITY: list[str] = [sys.executable, "-m", "beryl_tools.beryl3d.purity"]


def steps() -> list[Step]:
    """The lane's steps, in the order they run. Each check Beryl 3D gains is appended here."""
    cargo = [*BY, "b3d"]
    return [
        Step("fmt", [*cargo, "fmt", "--check"], paths.REPO),
        Step("clippy", [*cargo, "clippy"], paths.REPO),
        Step("cargo-test", [*cargo, "cargo-test"], paths.REPO),
        Step("gate", [*cargo, "gate"], paths.REPO),
        Step("scene-purity", [*PURITY, "scene-purity"], paths.REPO),
        Step("lints-match", [*PURITY, "lints-match"], paths.REPO),
        Step("data-names", [*PURITY, "data-names"], paths.REPO),
        # Beryl 3D's own timeline of every recording takes minutes, so the lane compares the
        # recorded fixtures only and the sweep compares everything (sweep.py).
        Step("timeline-parity", [*cargo, "timeline-parity", "--fixtures"], paths.REPO),
    ]


# Slow CI has no `beryl3d` lane, so a step the fast check cuts off at its hard limit is the
# sweep's to run (sweep.py).
DEFERRED_TO = "the Beryl 3D sweep (`just b3d sweep`), not slow CI"


def lane() -> Lane:
    return Lane(NAME, steps=steps(), deferred_to=DEFERRED_TO)
