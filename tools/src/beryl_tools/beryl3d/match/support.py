"""Which web style values Beryl 3D's `Style` can express, and how two values are compared
(docs/beryl-3d.md, "Matching the web interface's look").

Every value of a record's style has a shape, a short description of its form that leaves out
its numbers: "uniform" or "sides differ" for a border, "2 layers, 1 inset" for a box shadow,
"linear + url" for a background image, "drop-shadow + brightness" for a filter.
`data/beryl3d/match/supported.json` lists, for each property, the shapes Beryl 3D can express,
with "*" for every shape. A web value whose shape is not listed is unsupported: `match styles`
leaves it out of its differences, and `match gaps` lists it. A change that teaches `Style` a new
shape adds it to that file in the same commit.
"""

from __future__ import annotations

import json
import math
from collections.abc import Mapping, Sequence
from functools import cache
from pathlib import Path

from ... import paths as repo_paths
from . import css, records

SUPPORTED: Path = repo_paths.REPO / "data" / "beryl3d" / "match" / "supported.json"

# How far two values may differ and still count as the same: a color channel by one level of
# 255, an alpha or a ratio (opacity, a filter's amount, a transform's scale or rotation) by
# 0.005, which is what the records' rounding leaves, a length by a quarter of a point, and an
# angle by half a degree.
CHANNEL = 1.0
RATIO = 0.005
LENGTH = 0.25
ANGLE = 0.5

# The properties whose value is a color, or four sides' colors.
COLORS: frozenset[str] = frozenset({"background-color", "border-color", "color"})
LENGTHS: frozenset[str] = frozenset({"border-width", "border-radius", "font-size"})
LAYERED: frozenset[str] = frozenset({"background-image", "mask-image"})


@cache
def supported() -> Mapping[str, frozenset[str]]:
    table = json.loads(SUPPORTED.read_text())["supported"]
    return {name: frozenset(shapes) for name, shapes in table.items()}


def plural(count: int, word: str) -> str:
    return f"{count} {word}" if count == 1 else f"{count} {word}s"


def transform_shape(value: Sequence[object]) -> str:
    """A `matrix()` (6 numbers) or `matrix3d()` (16) by what it does."""
    if len(value) != 6:
        return "3d"
    a, b, c, d, _, _ = (float(str(v)) for v in value)
    if abs(b) > RATIO or abs(c) > RATIO:
        return "rotate or skew"
    if abs(a - 1) > RATIO or abs(d - 1) > RATIO:
        return "scale"
    return "translate"


def shape(name: str, value: object) -> str | None:
    """The shape of `value`, the web's value of style property `name`, or None when it draws
    nothing Beryl 3D would need to express ("none")."""
    if value == "none" or value is None:
        return None
    if name in COLORS:
        many = isinstance(value, list) and value and isinstance(value[0], list)
        return "sides differ" if many else "color"
    if name == "border-width":
        return "sides differ" if isinstance(value, list) else "uniform"
    if name == "border-radius":
        return "corners differ" if isinstance(value, list) else "uniform"
    if name in ("background-clip", "background-origin") and isinstance(value, str):
        boxes = css.commas(value)
        if boxes and all(box in ("border-box", "padding-box") for box in boxes):
            return "border-box or padding-box"
    if not isinstance(value, list):
        return "value"
    if name == "box-shadow":
        inset = sum(
            1 for layer in value if isinstance(layer, Mapping) and layer["inset"]
        )
        text = plural(len(value), "layer")
        return f"{text}, {inset} inset" if inset else text
    if name in LAYERED:
        return " + ".join(
            ("repeating-" if layer.get("repeating") else "") + str(layer.get("kind"))
            for layer in value
            if isinstance(layer, Mapping)
        )
    if name == "filter":
        return " + ".join(str(f.get("fn")) for f in value if isinstance(f, Mapping))
    if name == "transform":
        return transform_shape(value)
    return "value"


def can_express(name: str, value: object) -> bool:
    """Whether Beryl 3D's `Style` can express `value` of property `name`."""
    found = shape(name, value)
    if found is None:
        return True
    shapes = supported().get(name, frozenset())
    return "*" in shapes or found in shapes


def matters(name: str, style: Mapping[str, object]) -> bool:
    """Whether property `name` of `style` changes what is drawn: a border's color does not
    when the border has no width. The web computes a border color for every element (its text
    color), where Beryl 3D writes none for an element without a border."""
    if name == "border-color":
        width = style.get("border-width", 0)
        widths = width if isinstance(width, list) else [width]
        return any(isinstance(w, (int, float)) and w > 0 for w in widths)
    return True


def near(a: float, b: float, tolerance: float) -> bool:
    return math.isclose(a, b, abs_tol=tolerance + 1e-9)


def is_number(value: object) -> bool:
    return records.number(value)


def same_color(a: object, b: object) -> bool:
    if not (records.is_color(a) and records.is_color(b)):
        return a == b
    assert isinstance(a, list) and isinstance(b, list)
    return all(
        near(float(x), float(y), CHANNEL) for x, y in zip(a[:3], b[:3])
    ) and near(float(a[3]), float(b[3]), RATIO)


def same_list(a: object, b: object, kind: str) -> bool:
    if not (isinstance(a, list) and isinstance(b, list)) or len(a) != len(b):
        return False
    return all(same(x, y, kind) for x, y in zip(a, b))


def same_mapping(a: Mapping[str, object], b: Mapping[str, object]) -> bool:
    if a.keys() != b.keys():
        return False
    for key in a:
        if key == "color":
            kind = "color"
        elif key in ("x", "y", "blur", "spread", "at"):
            kind = "length"
        elif key == "shape":
            kind = "angle"
        elif key == "value":
            kind = "length" if a.get("fn") == "blur" else "ratio"
        elif key == "stops":
            kind = "structure"
        else:
            kind = "exact"
        if not same(a[key], b[key], kind):
            return False
    return True


def rotation(value: object) -> float | None:
    """The rotation in degrees of a transform, `"none"` or a 2D `matrix()`'s six numbers, or None
    for a 3D one.

    A transform is compared by its rotation alone. The web lays its cards out flat and turns
    them with CSS, where Beryl 3D poses each card in 3D and projects it (its records write the
    rotation of a card's top edge on screen with no scale), so a card's scale and skew have no
    counterpart, and its position and size are compared by the boxes."""
    if value == "none" or value is None:
        return 0.0
    if not isinstance(value, list) or len(value) != 6:
        return None
    a, b = float(str(value[0])), float(str(value[1]))
    return math.degrees(math.atan2(b, a))


def angle_between(a: float, b: float) -> float:
    """The smaller angle in degrees between two directions."""
    turn = abs(a - b) % 360.0
    return min(turn, 360.0 - turn)


def same(a: object, b: object, kind: str) -> bool:
    """Whether `a` and `b` are the same value within the tolerance of `kind`: "color",
    "length", "ratio", "angle", "transform" (by `rotation`), "exact" or "structure" (shadows,
    gradients and filters, compared field by field)."""
    if kind == "color":
        if isinstance(a, list) and a and isinstance(a[0], list):
            return same_list(a, b, "color")
        return same_color(a, b)
    if kind == "transform":
        turn_a, turn_b = rotation(a), rotation(b)
        if turn_a is None or turn_b is None:
            return a == b
        return near(angle_between(turn_a, turn_b), 0.0, ANGLE)
    if is_number(a) and is_number(b):
        assert isinstance(a, (int, float)) and isinstance(b, (int, float))
        tolerance = {"length": LENGTH, "ratio": RATIO, "angle": ANGLE}.get(kind, 0.0)
        return near(float(a), float(b), tolerance)
    if isinstance(a, list):
        return same_list(a, b, kind)
    if isinstance(a, Mapping) and isinstance(b, Mapping):
        return same_mapping(a, b)
    return a == b


def kind_of(name: str) -> str:
    """How property `name`'s values are compared (`same`)."""
    if name in COLORS:
        return "color"
    if name in LENGTHS:
        return "length"
    if name == "opacity":
        return "ratio"
    if name == "transform":
        return "transform"
    if name in ("font-family", "font-weight"):
        return "exact"
    return "structure"
