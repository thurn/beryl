"""`just b3d match gaps`: every web style value that Beryl 3D's `Style` cannot express yet, which
is what the style vocabulary has to learn (docs/beryl-3d.md, "Matching the web interface's
look").

Over one target's web elements, or with `--all` over every desktop and phone test target's, it
groups each unsupported value (support.py) by its property and shape, such as "box-shadow,
2 layers, 1 inset", and ranks the groups by the total screen area of the elements that use
them, with the three largest of those elements as examples. An element that is fully
transparent, or outside the window, adds no area.

It prints the groups and writes them as JSON, to the snapshot's `gaps.json` for one target and
to `$BERYL_CACHE/b3d/match/gaps.json` with `--all`:

    {"targets": [<test name>], "groups": [{"property", "shape", "elements", "area",
     "examples": [{"target", "element", "box", "value"}]}]}

`area` is in square points and `value` is the example's value as CSS-like text.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from pathlib import Path

from .. import paths as beryl3d_paths
from . import boxes, css, records, snapshot, support
from .target import Target, arguments, target

Element = Mapping[str, object]

# How many examples each group names.
EXAMPLES = 3
# How many characters of an example's description are kept: the web names a container by all
# the text inside it.
DESCRIBED = 80


def gap_arguments(parser: argparse.ArgumentParser) -> None:
    arguments(parser, every=True)
    parser.add_argument(
        "--all",
        action="store_true",
        help="every desktop and phone test target, grouped together",
    )
    parser.add_argument("--fresh", action="store_true", help="take the snapshots again")
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )


@dataclass(frozen=True)
class Use:
    """One web element's unsupported value."""

    target: str
    element: str
    box: list[float]
    value: str
    # Its area on screen, in square points.
    area: float


@dataclass
class Group:
    """The unsupported values of one property and shape."""

    name: str
    shape: str
    uses: list[Use] = field(default_factory=list)

    def area(self) -> float:
        return sum(u.area for u in self.uses)

    def examples(self) -> list[Use]:
        return sorted(self.uses, key=lambda u: -u.area)[:EXAMPLES]

    def document(self) -> dict[str, object]:
        return {
            "property": self.name,
            "shape": self.shape,
            "elements": len(self.uses),
            "area": round(self.area()),
            "examples": [
                {
                    "target": u.target,
                    "element": u.element,
                    "box": u.box,
                    "value": u.value,
                }
                for u in self.examples()
            ],
        }


def visible_area(element: Element, width: float, height: float) -> float:
    """The part of `element`'s box inside the window, in square points; none when it is fully
    transparent."""
    style = element.get("style")
    if isinstance(style, Mapping) and style.get("opacity") == 0:
        return 0.0
    x, y, w, h = boxes.box(element)
    across = max(0.0, min(x + w, width) - max(x, 0.0))
    down = max(0.0, min(y + h, height) - max(y, 0.0))
    return across * down


def short(text: str) -> str:
    return text if len(text) <= DESCRIBED else text[: DESCRIBED - 1] + "…"


def uses(t: Target, web: Iterable[Element]) -> list[tuple[str, str, Use]]:
    """Each unsupported value of `web`, the target's web elements, with its property and shape."""
    width, height = t.width_height()
    out: list[tuple[str, str, Use]] = []
    for element in web:
        style = element["style"]
        assert isinstance(style, Mapping)
        for name in records.STYLE:
            value = style.get(name)
            if (
                name not in style
                or not support.matters(name, style)
                or support.can_express(name, value)
            ):
                continue
            shape = support.shape(name, value)
            assert shape is not None
            out.append(
                (
                    name,
                    shape,
                    Use(
                        t.test_name(),
                        short(boxes.describe(element)),
                        list(boxes.box(element)),
                        css.text(value, name in support.COLORS),
                        visible_area(element, width, height),
                    ),
                )
            )
    return out


def grouped(found: Iterable[tuple[str, str, Use]]) -> list[Group]:
    """`found` grouped by property and shape, largest area first."""
    groups: dict[tuple[str, str], Group] = {}
    for name, shape, use in found:
        groups.setdefault((name, shape), Group(name, shape)).uses.append(use)
    return sorted(groups.values(), key=lambda g: (-g.area(), g.name, g.shape))


def measure(t: Target, directory: Path) -> list[tuple[str, str, Use]]:
    return uses(t, json.loads((directory / "web.json").read_text())["elements"])


def document(targets: list[str], groups: list[Group]) -> dict[str, object]:
    return {"targets": sorted(targets), "groups": [g.document() for g in groups]}


def printout(targets: list[str], groups: list[Group], written: Path) -> str:
    out = [
        f"web style values Beryl 3D cannot express, over {len(targets)} target(s), "
        f"largest area first ({len(groups)} groups):"
    ]
    for g in groups:
        out.append(
            f"  {g.name}, {g.shape}: {len(g.uses)} elements, {round(g.area())} pt²"
        )
        many = len(targets) > 1
        for u in g.examples():
            where = f"{u.target}  " if many else ""
            out.append(f"    {where}{u.element}  {boxes.shown(u.box)}  {u.value}")
    out.append(f"wrote {written}")
    return "\n".join(out) + "\n"


def write(path: Path, targets: list[str], groups: list[Group]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document(targets, groups), indent=1) + "\n")


def run_all(args: argparse.Namespace) -> int:
    results = snapshot.every(args.recording, args.at, args.fresh, args.art, measure)
    if results is None:
        return 1
    targets: list[str] = []
    found: list[tuple[str, str, Use]] = []
    failed = 0
    for t, result in results:
        if result is None:
            failed += 1
            print(
                f"{t.test_name()}: failed (the snapshot could not be taken)", flush=True
            )
            continue
        targets.append(t.test_name())
        found += result
    groups = grouped(found)
    path = beryl3d_paths.cache() / "match" / "gaps.json"
    write(path, targets, groups)
    print(printout(targets, groups, path), end="")
    if failed:
        print(f"{failed} targets failed", file=sys.stderr)
    return 1 if failed or not targets else 0


def run(args: argparse.Namespace) -> int:
    if args.all:
        return run_all(args)
    if not args.recording or not args.at:
        print("match gaps: give a recording and --at, or --all", file=sys.stderr)
        return 2
    t = target(args)
    directory = snapshot.ensure(t, fresh=args.fresh, art=args.art)
    if directory is None:
        return 1
    groups = grouped(measure(t, directory))
    path = directory / "gaps.json"
    write(path, [t.test_name()], groups)
    print(printout([t.test_name()], groups, path), end="")
    return 0
