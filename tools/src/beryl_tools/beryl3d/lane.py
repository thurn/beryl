"""The `beryl3d` lanes, which the fast check runs for a change to a Beryl 3D path and
`just b3d lane` runs by hand. Slow CI has no such lanes: Beryl 3D's sweep runs after a push
instead (docs/beryl-3d.md, 'Checks and isolation').

The steps are split into several lanes ([`GROUPS`]), which the fast check runs at once. Run one
after another in a single lane, they took more than the fast check's 180 s hard limit at the
machine's usual load: clippy about 20 s, the shared build, the recordings and the workspace's
tests 50-110 s, the gate 5-30 s, the interface tests 30-60 s and timeline parity 20-65 s. The
lane was cut off in 49 of 130 fast checks on 2026-11-15. None of the steps after the build
writes the build directory, so they can run side by side.
"""

from __future__ import annotations

import sys

from ..lanes import BY, Lane, Step
from .. import paths

NAME = "beryl3d"
PURITY: list[str] = [sys.executable, "-m", "beryl_tools.beryl3d.purity"]
CARGO: list[str] = [*BY, "b3d"]

# The first step of each lane that runs a program of the workspace: the build that the tests,
# `record` and `gate` share (`cli.SHARED_BUILD`), then the recordings pack. The lanes start it
# together, Cargo's lock on the build directory lets one of them build, and the others then
# find nothing to do. Without it, `test` and `timeline-parity` would start by building `b3d`
# alone (`cli.run_b3d`), with other features than the shared build, and the shared build would
# compile those crates again.
RECORD = Step("record", [*CARGO, "record"], paths.REPO)

# Each lane's name and its steps by name, in the order they run. The first lane holds the steps
# that build nothing in the shared build directory (clippy has a target directory of its own,
# `cli.CLIPPY_TARGET`); each other lane starts with [`RECORD`].
GROUPS: tuple[tuple[str, tuple[str, ...]], ...] = (
    (NAME, ("fmt", "clippy", "scene-purity", "lints-match", "data-names")),
    (f"{NAME}-tests", ("cargo-test", "gate")),
    (f"{NAME}-interface", ("test",)),
    (f"{NAME}-timeline", ("timeline-parity",)),
)


def steps() -> list[Step]:
    """Every step of the lanes, once each. The sweep runs each as a lane of its own (sweep.py).
    Each check Beryl 3D gains is added here and to a lane of [`GROUPS`]."""
    return [
        Step("fmt", [*CARGO, "fmt", "--check"], paths.REPO),
        Step("clippy", [*CARGO, "clippy"], paths.REPO),
        Step("cargo-test", [*CARGO, "cargo-test"], paths.REPO),
        Step("gate", [*CARGO, "gate"], paths.REPO),
        # Every interface test, with the checks applied at the check times (docs/beryl-3d.md,
        # "Test speed"). The sweep's `every-tick` step is its exhaustive form.
        Step("test", [*CARGO, "test"], paths.REPO),
        Step("scene-purity", [*PURITY, "scene-purity"], paths.REPO),
        Step("lints-match", [*PURITY, "lints-match"], paths.REPO),
        Step("data-names", [*PURITY, "data-names"], paths.REPO),
        # Beryl 3D's own timeline of every recording takes minutes, so the lane compares the
        # recorded fixtures only and the sweep compares everything (sweep.py).
        Step("timeline-parity", [*CARGO, "timeline-parity", "--fixtures"], paths.REPO),
    ]


# Slow CI has no `beryl3d` lanes, so a step the fast check cuts off at its hard limit is the
# sweep's to run (sweep.py).
DEFERRED_TO = "the Beryl 3D sweep (`just b3d sweep`), not slow CI"


def lanes() -> list[Lane]:
    """The lanes of [`GROUPS`], which the fast check runs at once."""
    by_name = {s.name: s for s in steps()}
    made: list[Lane] = []
    for n, (name, names) in enumerate(GROUPS):
        first = [] if n == 0 else [RECORD]
        made.append(
            Lane(
                name,
                steps=[*first, *(by_name[s] for s in names)],
                deferred_to=DEFERRED_TO,
            )
        )
    return made
