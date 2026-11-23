"""The committed web reference and the ratchet file, which `just b3d match score` and the
`b3d-web-match` and `b3d-web-drift` check families read (docs/beryl-3d.md, "Matching the web
interface's look").

- **The reference targets.** One desktop and one phone window of every screen kind in
  `data/beryl3d/screens.json`, each at the recording's end, seat `you` and scale 2, and the
  mid-game moments of `MOMENTS` in the desktop window. A target at its recording's end is
  named by its test's name, and one at another moment by its test's name and `#<moment>`
  (`entry_name`).
- **The reference.** `data/beryl3d/match/web/<slug>.json` holds the web interface's element
  records of each reference target, trimmed to what the score reads (`trim`) and written one
  element to a line, so that a change to the web interface shows as a short diff. A check that
  runs without WebKit compares Beryl 3D's records with it.
- **The ratchet file.** `data/beryl3d/web-match.json` holds each target's counts, which may only
  go down (`Counts`).

The counts of a target come from its two sides' records alone (`count`), without the pixel count,
which needs the web's picture.
"""

from __future__ import annotations

import argparse
import json
import re
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import paths as beryl3d_paths
from . import boxes, joined, styles
from .target import Target, target

Element = Mapping[str, Any]

REFERENCE: Path = beryl3d_paths.DATA / "match" / "web"
RATCHET: Path = beryl3d_paths.DATA / "web-match.json"
SCREENS: Path = beryl3d_paths.DATA / "screens.json"
# The moment every reference target is taken at, as `just b3d match <subcommand> --all` takes it.
AT = "end"
# The counts a target holds, in the order the ratchet file and reports list them.
COUNTS: tuple[str, ...] = ("unmatched", "boxes", "styles", "unsupported", "pixels")
# The counts that need only the two sides' records, which the `b3d-web-match` checks hold.
RECORD_COUNTS: tuple[str, ...] = COUNTS[:-1]
# The digits of a box kept in the reference: hundredths of a point, far below the half point
# the box comparison treats as equal (`boxes.EXACT`).
PLACES = 2


# The windows of the reference targets, as a test's name spells them: the desktop test that is
# drawn at a Retina display's scale, and the phone test (`b3d test --list` names both).
WINDOWS = (("desktop", "1600x900@2"), ("phone", "393x759"))


# Built screens excluded from web comparison, with the reason for each.
WITHOUT_REFERENCE = {
    "screen/testing": "a frame of 59467 records, over the renderer's 27728",
    "screen/gallery": "Beryl 3D's story browser has its own layout and content",
}

# Moments in the middle of a game, which no screen's end shows, each as a recording and a
# moment played at real time (`--no-catch-up`), held in the desktop window. Arrows and lines of
# combat show only mid-game, and every arrow once drew nothing while each target at a
# recording's end still matched.
MOMENTS: tuple[tuple[str, str], ...] = (
    # Sample about 68 ms after the mana streak lands on Counterspell, with both stack
    # lines still visible. The step keeps the sample with this cast when earlier animations
    # change duration.
    ("combat-attackers", "step:163.1.2+205ms"),
    # Blockers declared: an attacking token with its wedge, and the line of combat from the
    # creature that blocks it.
    ("combat-attackers", "118000ms"),
)


def targets() -> list[Target]:
    """The reference targets: for every screen kind, its recording in the desktop window and in
    the phone window, as the tests of `just b3d test --list` name them, and each of `MOMENTS`
    in the desktop window."""
    screens = json.loads(SCREENS.read_text())["screens"]
    found: list[Target] = []
    for screen in screens:
        if not screen.get("built") or screen["recording"] in WITHOUT_REFERENCE:
            continue
        for _, window in WINDOWS:
            found.append(named(f"b3d:{screen['recording']}/you/{window}"))
    desktop = dict(WINDOWS)["desktop"]
    for recording, at in MOMENTS:
        found.append(named(f"b3d:{recording}/you/{desktop}", at))
    return found


def named(test: str, at: str = AT) -> Target:
    """The target of the test named `test`, at `at`."""
    return target(
        argparse.Namespace(
            recording=test, at=at, seat=None, layout=None, size=None, scale=None
        )
    )


def entry_name(t: Target) -> str:
    """The target's name in the ratchet file: its test's name, with `#<moment>` after it for a
    target that is not at its recording's end."""
    return t.test_name() if t.at == AT else f"{t.test_name()}#{t.at}"


def check_name(t: Target) -> str:
    """The target's name as a check names it: its entry's name without the `b3d:`."""
    return entry_name(t).removeprefix("b3d:")


def by_check_name(name: str) -> Target | None:
    """The reference target a check name (`check_name`) stands for, or None."""
    return next((t for t in targets() if check_name(t) == name), None)


def path(t: Target) -> Path:
    """The target's reference file."""
    return REFERENCE / (re.sub(r"[^A-Za-z0-9.@-]+", "_", check_name(t)) + ".json")


def rounded(box: object) -> list[float]:
    return [round(float(v), PLACES) for v in joined.as_box(box)]


def trim(elements: Sequence[Element]) -> list[dict[str, Any]]:
    """`elements` with only the fields the score reads: no paint order, and no line of text
    beyond its string, type, baseline and box. Boxes are rounded to hundredths of a point.
    """
    out: list[dict[str, Any]] = []
    for e in elements:
        out.append(
            {
                "key": e.get("key"),
                "role": e.get("role"),
                "name": e.get("name"),
                "object": e.get("object"),
                "box": rounded(e["box"]),
                "parent": e.get("parent"),
                "style": e["style"],
                "text": [
                    {**line, "box": rounded(line["box"])}
                    for line in e.get("text") or []
                ],
            }
        )
    return out


def document(t: Target, elements: Sequence[Element]) -> str:
    """The reference file's text for `elements`: the target's head, then one element to a
    line."""
    head = {"target": t.test_name(), "scale": float(t.scale), "at": t.at}
    lines = [
        json.dumps(e, separators=(",", ":"), sort_keys=True) for e in trim(elements)
    ]
    body = ",\n".join(f"  {line}" for line in lines)
    return (
        json.dumps(head, separators=(",", ":"))[:-1] + f',"elements":[\n{body}\n]}}\n'
    )


def write(t: Target, elements: Sequence[Element]) -> Path:
    """Writes the web `elements` of `t` as its reference file."""
    REFERENCE.mkdir(parents=True, exist_ok=True)
    file = path(t)
    file.write_text(document(t, elements))
    return file


def load(t: Target) -> list[Element] | None:
    """The reference elements of `t`, or None when it has no reference file."""
    file = path(t)
    if not file.is_file():
        return None
    elements: list[Element] = json.loads(file.read_text())["elements"]
    return elements


def count_shared(web: Sequence[Element], b3d: Sequence[Element]) -> dict[str, int]:
    """Compare the shared UI. The web floats mana costs above hand cards, as documented
    in UI Conventions. Keep those records for web drift and omit their subtrees here.
    """
    shared: list[Element] = []
    kept: dict[int, int] = {}
    # The capture writes parents before children. Remap each parent to its retained index.
    for index, element in enumerate(web):
        parent = joined.parent(element)
        if element.get("role") == "hand-cost" or (
            parent is not None and parent not in kept
        ):
            continue
        kept[index] = len(shared)
        shared.append(
            {**element, "parent": kept[parent] if parent is not None else None}
        )
    return count(shared, b3d)


def count(web: Sequence[Element], b3d: Sequence[Element]) -> dict[str, int]:
    """The record counts of a target (`RECORD_COUNTS`) from its two sides' elements:

    - `unmatched`: elements the matcher left unpaired, on either side;
    - `boxes`: pairs whose boxes or lines differ, and pairs whose text overlaps on one side
      only;
    - `styles`: properties a pair draws differently;
    - `unsupported`: web values of a pair that `Style` cannot express.
    """
    found = joined.join(web, b3d)
    pairs = found["pairs"]
    unmatched = found["unmatched"]
    assert isinstance(pairs, list) and isinstance(unmatched, dict)
    only_b3d, only_web = boxes.one_sided_overlaps(web, b3d, pairs)
    style = styles.measure(web, b3d, pairs)
    return {
        "unmatched": len(unmatched["web"]) + len(unmatched["b3d"]),
        "boxes": len(boxes.differences(web, b3d, pairs))
        + len(only_b3d)
        + len(only_web),
        "styles": len(style.differences),
        "unsupported": sum(style.unsupported.values()),
    }


# How far the pixel count may move, as a fraction of its entry, before it counts as a change.
# The same snapshot always counts the same, but a web page shot while the machine is loaded
# differs from one shot at rest (a font or an image arriving a frame later): by 0.2% of a
# 5.5-million pixel picture in measured runs. The other counts are exact.
PIXEL_SLACK = 0.01


@dataclass(frozen=True)
class Counts:
    """A target's counts, or those of the ratchet file's entry for it."""

    values: Mapping[str, int]

    def rose(self, entry: "Counts", names: Sequence[str] = COUNTS) -> list[str]:
        """The counts among `names` that are above `entry`'s, as `boxes 14 > 12`."""
        out: list[str] = []
        for n in names:
            held = entry.values.get(n, 0)
            limit = held * (1 + PIXEL_SLACK) if n == "pixels" else held
            if n in self.values and self.values[n] > limit:
                out.append(f"{n} {self.values[n]} > {held}")
        return out

    def fell(self, entry: "Counts", names: Sequence[str] = COUNTS) -> list[str]:
        """The counts among `names` that are below `entry`'s, as `boxes 10 < 12`."""
        out: list[str] = []
        for n in names:
            held = entry.values.get(n, 0)
            limit = held * (1 - PIXEL_SLACK) if n == "pixels" else held
            if n in self.values and self.values[n] < limit:
                out.append(f"{n} {self.values[n]} < {held}")
        return out


def load_ratchet(file: Path = RATCHET) -> dict[str, Any]:
    """The ratchet file's document: `{"targets": {...}, "samples": {...}}`."""
    document: dict[str, Any] = json.loads(file.read_text())
    document.setdefault("targets", {})
    return document


def entry(document: Mapping[str, Any], name: str) -> Counts | None:
    """The ratchet entry of the test named `name`, or None when it is not listed."""
    held = document["targets"].get(name)
    if not isinstance(held, dict):
        return None
    return Counts({n: int(held[n]) for n in COUNTS if n in held})


def write_ratchet(document: Mapping[str, Any], file: Path = RATCHET) -> None:
    """Writes the ratchet file with its targets and samples sorted by name."""
    out = {
        "targets": dict(sorted(document["targets"].items())),
        "samples": dict(sorted(document.get("samples", {}).items())),
    }
    file.write_text(json.dumps(out, indent=1) + "\n")
