"""The `beryl3d` lane, which the fast check runs for a change to a Beryl 3D path and
`just b3d lane` runs by hand. Slow CI has no such lane: Beryl 3D's sweep runs after a push
instead (docs/beryl-3d.md, 'Checks and isolation')."""

from __future__ import annotations

from ..lanes import Lane, Step

NAME = "beryl3d"


def steps() -> list[Step]:
    """The lane's steps, in the order they run. Each check Beryl 3D gains is appended here."""
    return []


def lane() -> Lane:
    return Lane(NAME, steps=steps())
