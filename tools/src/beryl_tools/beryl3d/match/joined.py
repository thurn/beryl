"""The matcher: pairs each element of the web interface's records (`web.json`) with the Beryl 3D
element that stands for it (`b3d.json`), and writes the pairs and what is left over on each side
to the snapshot's `joined.json` (docs/beryl-3d.md, "Matching the web interface's look").

Three passes pair the elements, each over what the passes before it left:

1. **Key.** Elements whose locators (`key`) are equal.
2. **Identity.** Elements with the same role, name and object (a card's slot in the view) under
   the same parent. Only elements with a name or an object take part, because the web keeps many
   unnamed wrappers that would pair with anything. The two trees nest differently, so "the same
   parent" means that the nearest ancestors already paired on each side are each other's pair, or
   that neither side has one. Of several candidates, the one with the nearest box wins.
3. **Box.** Elements with the same role whose boxes lie within `TOLERANCE` points of each other,
   nearest first.

`joined.json` is `{"target", "pairs": [{"web", "b3d", "pass"}], "unmatched": {"web": [...],
"b3d": [...]}}`: `web` and `b3d` are indexes into each side's elements, `pass` is "key",
"identity" or "box", and each unmatched element is `{"index", "role", "name", "box"}`.
"""

from __future__ import annotations

import json
import math
from collections.abc import Mapping, Sequence
from pathlib import Path

Element = Mapping[str, object]

# How far apart, in points, two boxes may be and still pair in the box pass: the larger of the
# distance between their centers and the difference in their widths or heights. A label moved a
# few points or a panel a little wider still pairs; a different control of the same role
# elsewhere does not.
TOLERANCE = 24.0


def as_box(value: object) -> tuple[float, float, float, float]:
    """`value`, a record's `[x, y, width, height]` (records.py checks the contract)."""
    assert isinstance(value, list) and len(value) == 4
    x, y, width, height = (float(v) for v in value if isinstance(v, (int, float)))
    return x, y, width, height


def box(element: Element) -> tuple[float, float, float, float]:
    return as_box(element["box"])


def index(pair: Mapping[str, object], side: str) -> int:
    """The index of `pair`'s element on `side`, "web" or "b3d"."""
    value = pair[side]
    assert isinstance(value, int)
    return value


def distance(a: Element, b: Element) -> float:
    """How far apart the boxes of `a` and `b` are, in points: the larger of the distance between
    their centers and the difference in their widths or heights."""
    ax, ay, aw, ah = box(a)
    bx, by, bw, bh = box(b)
    centers = math.hypot(ax + aw / 2 - (bx + bw / 2), ay + ah / 2 - (by + bh / 2))
    return max(centers, abs(aw - bw), abs(ah - bh))


def parent(element: Element) -> int | None:
    value = element.get("parent")
    return value if isinstance(value, int) else None


def anchor(
    elements: Sequence[Element], index: int, paired: Mapping[int, int]
) -> int | None:
    """The index of the nearest ancestor of `elements[index]` that `paired` holds, or None."""
    up = parent(elements[index])
    while up is not None:
        if up in paired:
            return up
        up = parent(elements[up])
    return None


def identity(element: Element) -> tuple[object, ...] | None:
    """What the identity pass compares, or None for an element with neither name nor object."""
    name, thing = element.get("name") or "", element.get("object")
    if not name and thing is None:
        return None
    return (element.get("role"), name, thing)


def join(web: Sequence[Element], b3d: Sequence[Element]) -> dict[str, object]:
    """The pairs of `web` and `b3d` elements and the elements left over, as `joined.json` holds
    them, without its target."""
    pairs: list[dict[str, object]] = []
    # Each side's index to the other's, for the elements paired so far.
    web_to: dict[int, int] = {}
    b3d_to: dict[int, int] = {}

    def pair(w: int, b: int, how: str) -> None:
        web_to[w] = b
        b3d_to[b] = w
        pairs.append({"web": w, "b3d": b, "pass": how})

    keyed = {e["key"]: i for i, e in enumerate(b3d) if e.get("key")}
    for w, element in enumerate(web):
        b = keyed.get(element.get("key"))
        if b is not None and b not in b3d_to:
            pair(w, b, "key")

    # Parents come before their children in both documents, so walking the web's elements in
    # order pairs a parent before the identity pass asks about its children's anchors.
    for w, element in enumerate(web):
        wanted = identity(element)
        if w in web_to or wanted is None:
            continue
        up = anchor(web, w, web_to)
        place = None if up is None else web_to[up]
        candidates = [
            b
            for b, other in enumerate(b3d)
            if b not in b3d_to
            and identity(other) == wanted
            and anchor(b3d, b, b3d_to) == place
        ]
        if candidates:
            pair(
                w, min(candidates, key=lambda b: distance(element, b3d[b])), "identity"
            )

    near = sorted(
        (distance(we, be), w, b)
        for w, we in enumerate(web)
        if w not in web_to
        for b, be in enumerate(b3d)
        if b not in b3d_to and we.get("role") == be.get("role")
    )
    for apart, w, b in near:
        if apart > TOLERANCE:
            break
        if w not in web_to and b not in b3d_to:
            pair(w, b, "box")

    def left(elements: Sequence[Element], paired: Mapping[int, int]) -> list[object]:
        return [
            {
                "index": i,
                "role": e.get("role"),
                "name": e.get("name"),
                "box": e.get("box"),
            }
            for i, e in enumerate(elements)
            if i not in paired
        ]

    return {
        "pairs": pairs,
        "unmatched": {"web": left(web, web_to), "b3d": left(b3d, b3d_to)},
    }


def ensure(directory: Path) -> dict[str, object]:
    """Pairs the snapshot in `directory` and writes its `joined.json`; returns what it wrote.
    Pairing takes milliseconds, so it is done again on every call rather than cached."""
    web = json.loads((directory / "web.json").read_text())
    b3d = json.loads((directory / "b3d.json").read_text())
    joined = {"target": b3d["target"], **join(web["elements"], b3d["elements"])}
    (directory / "joined.json").write_text(json.dumps(joined, indent=1) + "\n")
    return joined
