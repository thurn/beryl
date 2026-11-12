"""The suites `ui-test` runs beside the checks of interactions.mjs and kept.mjs, each with its
own run-log prefix and a list of its checks Python can read: the stage's verbs
(ui/scripts/verbs.mjs, ui/verbs.json) and its prompt shapes (ui/scripts/shapes.mjs,
ui/shapes.json). selection.py chooses among them, and shots.py and motion.py record their
fixtures."""

from __future__ import annotations

import json

from . import paths

# The verb suite, which `ui-test` runs beside the checks: its script, its cases, the list
# of its checks, and the prefix of their ids in the run log (the lane's others are `ui-test:<title>`).
VERB_SCRIPT = "ui/scripts/verbs.mjs"
VERB_CASES = (
    "ui/scripts/verb-cases.mjs",
    "ui/scripts/verb-cases-place.mjs",
    "ui/scripts/verb-cases-piles.mjs",
    "ui/scripts/verb-cases-spans.mjs",
    "ui/scripts/verb-cases-rest.mjs",
    "ui/scripts/verb-spans.mjs",
    "ui/scripts/verb-rest.mjs",
    "ui/scripts/written-marks.mjs",
    "ui/scripts/verb-audit.mjs",
    "ui/scripts/stage-audit.mjs",
)
VERBS = "ui/verbs.json"
VERB_PREFIX = "verb:"
# The shape suite, which `ui-test` runs after it: its script, its cases, the list of its checks,
# and the prefix of their ids in the run log.
SHAPE_SCRIPT = "ui/scripts/shapes.mjs"
SHAPE_CASES: tuple[str, ...] = tuple(
    f"ui/scripts/shape-cases{part}.mjs" for part in ("", "-table", "-panel")
)
SHAPES = "ui/shapes.json"
SHAPE_PREFIX = "shape:"


def verb_suite() -> dict[str, object]:
    """ui/verbs.json: the verb suite's verbs, layouts and the fixtures of its moment by seat.
    Empty where a checkout has none."""
    try:
        found = json.loads((paths.REPO / VERBS).read_text())
    except (OSError, ValueError):
        return {}
    return found if isinstance(found, dict) else {}


def verb_checks() -> list[str]:
    """The verb suite's checks, by their run-log ids."""
    suite = verb_suite()
    verbs, layouts, moment = (suite.get(k) for k in ("verbs", "layouts", "moment"))
    if not (
        isinstance(verbs, list)
        and isinstance(layouts, list)
        and isinstance(moment, dict)
    ):
        return []
    return [
        f"{VERB_PREFIX}{verb}/{seat}/{layout}"
        for verb in verbs
        for seat in moment
        for layout in layouts
    ]


def verb_fixtures() -> list[str]:
    """The fixtures the verb suite plays: its first moment's, whole and cut, and those of the
    moments its verbs watch besides (`more`), one a seat."""
    suite = verb_suite()
    moment, more = suite.get("moment"), suite.get("more")
    seats = list(moment.values()) if isinstance(moment, dict) else []
    seats += list(more.values()) if isinstance(more, dict) else []
    return sorted({str(f) for s in seats if isinstance(s, dict) for f in s.values()})


def shape_suite() -> dict[str, object]:
    """ui/shapes.json: the shape suite's shapes, each with its fixture by seat, and its layouts.
    Empty where a checkout has none."""
    try:
        found = json.loads((paths.REPO / SHAPES).read_text())
    except (OSError, ValueError):
        return {}
    return found if isinstance(found, dict) else {}


def shape_checks() -> list[str]:
    """The shape suite's checks, by their run-log ids."""
    suite = shape_suite()
    shapes, layouts = suite.get("shapes"), suite.get("layouts")
    if not (isinstance(shapes, dict) and isinstance(layouts, list)):
        return []
    return [
        f"{SHAPE_PREFIX}{shape}/{seat}/{layout}"
        for shape, seats in shapes.items()
        if isinstance(seats, dict)
        for seat in seats
        for layout in layouts
    ]


def shape_fixtures() -> list[str]:
    """The fixtures the shape suite plays: each shape's, one a seat."""
    shapes = shape_suite().get("shapes")
    seats = list(shapes.values()) if isinstance(shapes, dict) else []
    return sorted({str(f) for s in seats if isinstance(s, dict) for f in s.values()})
