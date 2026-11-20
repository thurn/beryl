"""`just b3d match styles`: the style properties on which the two clients draw the same element
differently at one moment (docs/beryl-3d.md, "Matching the web interface's look").

For each pair of elements the matcher made (joined.py), it compares each property of the
contract (records.py) and prints one line per difference, largest element first:

    <element>  <property>  <web value>  <Beryl 3D value>

Values are compared normalized (support.py): colors within one level per channel, lengths
within a quarter of a point, and shadows, gradients and filters field by field. A web value
`Style` cannot express is not a difference: it is counted at the end, and `match gaps` lists
it. The type properties (`color` and the font's) are compared only where either side has text,
since the web computes them for every element.
"""

from __future__ import annotations

import argparse
import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from . import boxes, css, joined, records, snapshot, support
from .target import Target, arguments, target

Element = Mapping[str, object]


def style_arguments(parser: argparse.ArgumentParser) -> None:
    arguments(parser)
    parser.add_argument("--fresh", action="store_true", help="take the snapshot again")
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )


@dataclass(frozen=True)
class Difference:
    """One property that a pair of elements draws differently."""

    element: str
    name: str
    web: object
    b3d: object
    # The web element's area in square points, which orders the list.
    area: float

    def line(self) -> str:
        is_color = self.name in support.COLORS
        return (
            f"{self.element}  {self.name}  {css.text(self.web, is_color)}  "
            f"{css.text(self.b3d, is_color)}"
        )


@dataclass(frozen=True)
class Found:
    """What `styles` found for one target."""

    differences: list[Difference]
    # The paired elements' web values that `Style` cannot express, by property.
    unsupported: dict[str, int]

    def total(self) -> str:
        return (
            f"styles {len(self.differences)}, "
            f"unsupported {sum(self.unsupported.values())}"
        )


def area(element: Element) -> float:
    _, _, width, height = joined.box(element)
    return max(0.0, width) * max(0.0, height)


def compared(web: Element, b3d: Element) -> list[str]:
    """The properties to compare on a pair: every one both styles hold, but the type
    properties only where either side has text, and a border's color only where both sides
    have a border (a difference in width is listed on its own)."""
    web_style, b3d_style = web["style"], b3d["style"]
    assert isinstance(web_style, Mapping) and isinstance(b3d_style, Mapping)
    texty = bool(web.get("text")) or bool(b3d.get("text"))
    return [
        name
        for name in records.STYLE
        if name in web_style
        and name in b3d_style
        and (texty or name not in records.TYPE)
        and support.matters(name, web_style)
        and support.matters(name, b3d_style)
    ]


def compare_pair(web: Element, b3d: Element) -> tuple[list[Difference], list[str]]:
    """The differing properties of a pair, and the properties whose web value `Style` cannot
    express."""
    web_style, b3d_style = web["style"], b3d["style"]
    assert isinstance(web_style, Mapping) and isinstance(b3d_style, Mapping)
    found: list[Difference] = []
    unsupported: list[str] = []
    for name in compared(web, b3d):
        w, b = web_style[name], b3d_style[name]
        if support.same(w, b, support.kind_of(name)):
            continue
        if not support.can_express(name, w):
            unsupported.append(name)
            continue
        found.append(Difference(boxes.describe(b3d), name, w, b, area(web)))
    return found, unsupported


def matrix(value: object) -> list[float] | None:
    """A 2D transform's six numbers, the identity for "none", or None for a 3D one."""
    if value == "none" or value is None:
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    if isinstance(value, list) and len(value) == 6:
        return [float(str(v)) for v in value]
    return None


def as_drawn(web: Sequence[Element], at: int) -> Element:
    """Web element `at` with the transform its look is drawn with. A card's face on the web is a
    child region of the card's box, which a tapped card turns by itself, while the card element
    keeps only its scale; a card is compared by its own transform composed with that child's, as
    Beryl 3D's card is the face. Any other element is itself."""
    element = web[at]
    if element.get("role") != "card":
        return element
    style = element["style"]
    assert isinstance(style, Mapping)
    own = matrix(style.get("transform"))
    face = next(
        (
            child
            for child in web
            if child.get("parent") == at
            and child.get("role") == "region"
            and all(
                support.near(x, y, 1.0)
                for x, y in zip(joined.box(child), joined.box(element))
            )
        ),
        None,
    )
    if face is None or own is None:
        return element
    face_style = face["style"]
    assert isinstance(face_style, Mapping)
    inner = matrix(face_style.get("transform"))
    if inner is None or inner[:4] == [1.0, 0.0, 0.0, 1.0]:
        return element
    a, b, c, d, e, f = own
    a2, b2, c2, d2, _, _ = inner
    composed = [
        a * a2 + c * b2,
        b * a2 + d * b2,
        a * c2 + c * d2,
        b * c2 + d * d2,
        e,
        f,
    ]
    return {**element, "style": {**style, "transform": [round(v, 4) for v in composed]}}


def upright(element: Element) -> Element:
    """A card showing its back (it has no name) with its rotation folded by half a turn into a
    quarter turn either way of upright, since a back reads the same either way up: the web's
    desktop layout turns the opponent's hand half a turn and its phone layout does not, while
    Beryl 3D always poses those cards facing their owner. Any other element is itself.
    """
    style = element["style"]
    assert isinstance(style, Mapping)
    numbers = matrix(style.get("transform"))
    if element.get("role") != "card" or element.get("name") or numbers is None:
        return element
    turn = support.rotation(numbers)
    if turn is None or abs(turn) <= 90.0:
        return element
    a, b, c, d, e, f = numbers
    return {**element, "style": {**style, "transform": [-a, -b, -c, -d, e, f]}}


def measure(
    web: Sequence[Element],
    b3d: Sequence[Element],
    pairs: Sequence[Mapping[str, object]],
) -> Found:
    differences: list[Difference] = []
    unsupported: dict[str, int] = {}
    for p in pairs:
        found, missing = compare_pair(
            upright(as_drawn(web, joined.index(p, "web"))),
            upright(b3d[joined.index(p, "b3d")]),
        )
        differences += found
        for name in missing:
            unsupported[name] = unsupported.get(name, 0) + 1
    differences.sort(key=lambda d: (-d.area, d.element, records.STYLE.index(d.name)))
    return Found(differences, unsupported)


def find(directory: Path) -> Found:
    """Pairs the snapshot in `directory` (writing its `joined.json`) and compares the pairs'
    styles."""
    web = json.loads((directory / "web.json").read_text())["elements"]
    b3d = json.loads((directory / "b3d.json").read_text())["elements"]
    pairs = joined.ensure(directory)["pairs"]
    assert isinstance(pairs, list)
    return measure(web, b3d, pairs)


def printout(t: Target, found: Found) -> str:
    missing = ", ".join(f"{name} {n}" for name, n in sorted(found.unsupported.items()))
    out = [
        f"target {t.test_name()} ({t.layout}) at {t.at}",
        f"styles that differ ({len(found.differences)}):",
        *(f"  {d.line()}" for d in found.differences),
        f"web values Beryl 3D cannot express ({sum(found.unsupported.values())}): "
        f"{missing or 'none'} (`just b3d match gaps` lists them)",
        f"total: {found.total()}",
    ]
    return "\n".join(out) + "\n"


def run(args: argparse.Namespace) -> int:
    t = target(args)
    directory = snapshot.ensure(t, fresh=args.fresh, art=args.art)
    if directory is None:
        return 1
    print(printout(t, find(directory)), end="")
    return 0
