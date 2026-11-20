"""The matcher: pairs each element of the web interface's records (`web.json`) with the Beryl 3D
element that stands for it (`b3d.json`), and writes the pairs and what is left over on each side
to the snapshot's `joined.json` (docs/beryl-3d.md, "Matching the web interface's look").

First, each side's wrappers are left out: an element that draws nothing of its own (no
background, border, shadow, filter, mask or text) and whose role is one no player would name
(`WRAPPERS`, such as a `region` that only lays out its children). Neither side's wrappers have
anything to compare, and the two clients nest their elements differently, so they would only
count as unmatched.

An effect (role `effect`: light or a change of surface on a card, an ability or a player, or
the table shaking) pairs next, only by its name and what it is on (its `object`), since the two
sides draw an effect in different shapes and nest it differently. It takes part in no other
pass.

Six passes then pair the rest, each over what the passes before it left:

1. **Object.** Cards of the same object (a card's slot in the view, which both clients name
   their cards by), whatever their names or places. Where the web draws two cards of one
   object (during the opening, the stage's card below the window and the opening's copy in
   its row), the one on the screen pairs first.
2. **Mark.** Marks' outlines (role `mark`) of the same kind (their name: `eligible`,
   `selected`, `suggested`, `friendly-target`, `hostile-target`) on the same object. A mark
   pairs in this pass or not at all, so an outline one side lacks on a card is reported on that
   card ("only the web has this element") rather than paired with another card's outline or
   another kind of mark nearby, and a wrong one shows in `match styles`.
3. **Key.** Elements whose locators (`key`) are equal.
4. **Identity.** Elements with the same role, name and object under the same parent. Only
   elements with a name or an object take part, because the web keeps many unnamed elements
   that would pair with anything. The two trees nest differently, so "the same parent" means
   that the nearest ancestors already paired on each side are each other's pair, or that
   neither side has one. Of several candidates, the one with the nearest box wins.
5. **Identity anywhere.** Elements with the same role, name and object whose parents differ,
   nearest first: the web draws a hand card's cost in an element over the whole hand, while
   Beryl 3D draws it inside the card.
6. **Box.** Elements with the same role whose boxes lie within `TOLERANCE` points of each other,
   nearest first.

`joined.json` is `{"target", "pairs": [{"web", "b3d", "pass"}], "unmatched": {"web": [...],
"b3d": [...]}, "wrappers": {"web": n, "b3d": n}}`: `web` and `b3d` are indexes into each side's
elements, `pass` is "effect", "object", "mark", "key", "identity", "anywhere" or "box", each unmatched element is
`{"index", "role", "name", "box", "reason"}`, with `reason` saying why no pass paired it, and
`wrappers` counts the elements left out on each side.
"""

from __future__ import annotations

import json
import math
from collections.abc import Callable, Mapping, Sequence
from pathlib import Path

Element = Mapping[str, object]

# How far apart, in points, two boxes may be and still pair in the box pass: the larger of the
# distance between their centers and the difference in their widths or heights. A label moved a
# few points or a panel a little wider still pairs; a different control of the same role
# elsewhere does not.
TOLERANCE = 24.0

# The roles of elements that a player would not name, which are left out of matching when they
# draw nothing of their own: a `region` that only lays out its children, and a `list` that only
# holds its rows.
WRAPPERS: frozenset[str] = frozenset({"region", "list"})

# The role of a mark's outline on a card, an ability or a player, on both sides.
MARK = "mark"


def visible(color: object) -> bool:
    """Whether `color`, a record's `[r, g, b, a]` or null, can be seen."""
    return (
        isinstance(color, list)
        and len(color) == 4
        and isinstance(color[3], (int, float))
        and color[3] > 0
    )


def draws(element: Element) -> bool:
    """Whether `element` draws anything of its own: a background, a border, a shadow, a filter,
    a mask or text."""
    style = element.get("style")
    if element.get("text"):
        return True
    if not isinstance(style, Mapping):
        return False
    if visible(style.get("background-color")):
        return True
    if any(
        style.get(name, "none") not in ("none", None, [])
        for name in ("background-image", "box-shadow", "filter", "mask-image")
    ):
        return True
    # Four sides' widths and colors, or one of each for all four.
    widths, colors = style.get("border-width"), style.get("border-color")
    if not isinstance(widths, list):
        widths = [widths] * 4
    if not (isinstance(colors, list) and colors and isinstance(colors[0], list)):
        colors = [colors] * 4
    return any(
        isinstance(width, (int, float)) and width > 0 and visible(color)
        for width, color in zip(widths, colors)
    )


def wrapper(element: Element) -> bool:
    """Whether `element` is left out of matching: it draws nothing, and its role is one a player
    would not name."""
    return element.get("role") in WRAPPERS and not draws(element)


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
    web_out: set[int] = {i for i, e in enumerate(web) if wrapper(e)}
    b3d_out: set[int] = {i for i, e in enumerate(b3d) if wrapper(e)}

    def open_web(w: int) -> bool:
        return w not in web_to and w not in web_out and effect_of(web[w]) is None

    def open_b3d(b: int) -> bool:
        return b not in b3d_to and b not in b3d_out and effect_of(b3d[b]) is None

    # A mark pairs only in the mark pass; the later passes leave it out.
    mark_pass_done: bool = False

    def open_after_marks(element: Element) -> bool:
        return not (mark_pass_done and element.get("role") == MARK)

    def pair(w: int, b: int, how: str) -> None:
        web_to[w] = b
        b3d_to[b] = w
        pairs.append({"web": w, "b3d": b, "pass": how})

    effects = {effect_of(e): b for b, e in enumerate(b3d) if effect_of(e) is not None}
    for w, element in enumerate(web):
        b = effects.get(effect_of(element))
        if effect_of(element) is not None and b is not None and b not in b3d_to:
            pair(w, b, "effect")

    def nearest_first(
        how: str, alike: Callable[[Element, Element], bool], within: float
    ) -> None:
        """Pairs the open elements that are `alike`, nearest boxes first, up to `within`."""
        near = sorted(
            (distance(we, be), w, b)
            for w, we in enumerate(web)
            if open_web(w) and open_after_marks(we)
            for b, be in enumerate(b3d)
            if open_b3d(b) and open_after_marks(be) and alike(we, be)
        )
        for apart, w, b in near:
            if apart > within:
                break
            if open_web(w) and open_b3d(b):
                pair(w, b, how)

    # During the opening the web draws each card of the viewer's hand twice under one object:
    # the stage's card, moved below the window, and the opening's copy in its row. Beryl 3D
    # draws one, so a web card a player can see pairs first, and one off the screen pairs only
    # with what is left.
    screen = screen_of(web)
    nearest_first(
        "object",
        lambda we, be: same_card(we, be) and on_screen(we, screen),
        math.inf,
    )
    nearest_first("object", same_card, math.inf)
    nearest_first("mark", same_mark, math.inf)
    mark_pass_done = True

    keyed = {e["key"]: i for i, e in enumerate(b3d) if e.get("key")}
    for w, element in enumerate(web):
        b = keyed.get(element.get("key"))
        if (
            b is not None
            and open_web(w)
            and open_b3d(b)
            and element.get("role") != MARK
        ):
            pair(w, b, "key")

    # Parents come before their children in both documents, so walking the web's elements in
    # order pairs a parent before the identity pass asks about its children's anchors.
    for w, element in enumerate(web):
        wanted = identity(element)
        if not open_web(w) or wanted is None or element.get("role") == MARK:
            continue
        up = anchor(web, w, web_to)
        place = None if up is None else web_to[up]
        candidates = [
            b
            for b, other in enumerate(b3d)
            if open_b3d(b)
            and identity(other) == wanted
            and anchor(b3d, b, b3d_to) == place
        ]
        if candidates:
            pair(
                w, min(candidates, key=lambda b: distance(element, b3d[b])), "identity"
            )

    nearest_first(
        "anywhere",
        lambda we, be: identity(we) is not None and identity(we) == identity(be),
        math.inf,
    )
    nearest_first("box", lambda we, be: we.get("role") == be.get("role"), TOLERANCE)

    def left(side: str) -> list[object]:
        mine, paired, out = (
            (web, web_to, web_out) if side == "web" else (b3d, b3d_to, b3d_out)
        )
        others, taken, gone = (
            (b3d, b3d_to, b3d_out) if side == "web" else (web, web_to, web_out)
        )
        unpaired = [o for j, o in enumerate(others) if j not in taken and j not in gone]
        elsewhere = [o for j, o in enumerate(others) if j in taken]
        other = "Beryl 3D" if side == "web" else "web"
        mine_name = "web" if side == "web" else "Beryl 3D"
        return [
            {
                "index": i,
                "role": e.get("role"),
                "name": e.get("name"),
                "box": e.get("box"),
                "reason": (
                    lone_mark(e, mine, mine_name, other)
                    if e.get("role") == MARK
                    else reason(e, unpaired, elsewhere, other)
                ),
            }
            for i, e in enumerate(mine)
            if i not in paired and i not in out
        ]

    return {
        "pairs": pairs,
        "unmatched": {"web": left("web"), "b3d": left("b3d")},
        "wrappers": {"web": len(web_out), "b3d": len(b3d_out)},
    }


def effect_of(element: Element) -> tuple[object, object] | None:
    """An effect's name and what it is on, which the effect pass pairs by; None for an element
    that is not an effect."""
    if element.get("role") != "effect":
        return None
    return (element.get("name"), element.get("object"))


def same_card(a: Element, b: Element) -> bool:
    """Whether `a` and `b` are both cards of the same object."""
    return (
        a.get("role") == b.get("role") == "card"
        and a.get("object") is not None
        and a.get("object") == b.get("object")
    )


def screen_of(elements: Sequence[Element]) -> tuple[float, float, float, float] | None:
    """The box of the screen the `elements` are drawn on: the union of the boxes of the
    elements with no parent, which on the web are the page's top-level nodes. None when there
    are none."""
    roots = [box(e) for e in elements if parent(e) is None]
    if not roots:
        return None
    left = min(x for x, _, _, _ in roots)
    top = min(y for _, y, _, _ in roots)
    right = max(x + w for x, _, w, _ in roots)
    bottom = max(y + h for _, y, _, h in roots)
    return left, top, right - left, bottom - top


def on_screen(
    element: Element, screen: tuple[float, float, float, float] | None
) -> bool:
    """Whether some of `element`'s box lies on `screen` (everything does when it is None)."""
    if screen is None:
        return True
    x, y, w, h = box(element)
    sx, sy, sw, sh = screen
    return x < sx + sw and x + w > sx and y < sy + sh and y + h > sy


def same_mark(a: Element, b: Element) -> bool:
    """Whether `a` and `b` are both outlines of the same mark on the same object."""
    return (
        a.get("role") == b.get("role") == MARK
        and a.get("name") == b.get("name")
        and a.get("object") == b.get("object")
    )


def lone_mark(
    element: Element, elements: Sequence[Element], side: str, other: str
) -> str:
    """Why the mark `element`, one of `side`'s `elements`, is unpaired: `other` draws no such
    mark on its object, which is what an outline missing on one side looks like. It names the
    card, ability or player the mark rings, its parent."""
    up = parent(element)
    ringed = elements[up] if up is not None else {}
    thing = element.get("object")
    on = f"{ringed.get('role', 'thing')} {str(ringed.get('name') or '')!r}"
    if thing is not None:
        on += f" (object {thing})"
    return (
        f"only {'the web' if side == 'web' else side} has this element: the {other} side "
        f"draws no "
        f"{element.get('name')} mark on {on}"
    )


def reason(
    element: Element,
    unpaired: Sequence[Element],
    paired: Sequence[Element],
    other: str,
) -> str:
    """Why no pass paired `element`, given the `other` side's elements still `unpaired` and
    those already `paired` with something else."""
    role = element.get("role")
    alike = [o for o in unpaired if o.get("role") == role]
    if alike:
        nearest = min(alike, key=lambda o: distance(element, o))
        return (
            f"the nearest unpaired {other} {role}, {str(nearest.get('name'))[:40]!r}, is "
            f"{distance(element, nearest):.0f} points away, over {TOLERANCE:.0f}"
        )
    if any(o.get("role") == role for o in paired):
        return f"every {other} {role} is paired with another element"
    return f"the {other} side draws no {role}"


def ensure(directory: Path) -> dict[str, object]:
    """Pairs the snapshot in `directory` and writes its `joined.json`; returns what it wrote.
    Pairing takes milliseconds, so it is done again on every call rather than cached."""
    web = json.loads((directory / "web.json").read_text())
    b3d = json.loads((directory / "b3d.json").read_text())
    joined = {"target": b3d["target"], **join(web["elements"], b3d["elements"])}
    (directory / "joined.json").write_text(json.dumps(joined, indent=1) + "\n")
    return joined
