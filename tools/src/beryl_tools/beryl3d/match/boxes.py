"""`just b3d match boxes`: where the two clients lay the same element out differently at one
moment (docs/beryl-3d.md, "Matching the web interface's look").

It pairs the snapshot's elements (joined.py) and prints, worst first:

- each pair whose boxes differ by more than `EXACT` points on any side, or whose numbers of
  text lines differ when both have text: the element, the web's box, Beryl 3D's box, the
  four differences, and `WRAP n/m` when the same words are broken into a different number
  of lines or `LINES n/m` when the lines say different things (n the web's count, m Beryl
  3D's);
- the elements the matcher left unpaired on each side;
- the pairs of elements whose lines of text overlap on one side and not the other, which is
  how text drawn over other text shows up.

`--all` does this for every desktop and phone test target at once and prints each one's
totals.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from . import joined, snapshot
from .target import Target, arguments, target

Element = Mapping[str, object]
Box = tuple[float, float, float, float]

# How far, in points, two boxes may differ on any side and still count as the same: less
# than half a point is below what a Retina display shows as a move.
EXACT = 0.5
# How far two lines' boxes may reach into each other and not count as overlapping, in points,
# as in Beryl 3D's "text overlaps nothing" check (beryl_scene's `agent::text_overlap`): a
# line's box is as tall as its line, leading and all, so two lines set close together touch
# where no glyph does.
SLACK = 1.0
# How many unpaired elements each side lists before giving a count of the rest.
LISTED = 30


def box_arguments(parser: argparse.ArgumentParser) -> None:
    arguments(parser, every=True)
    parser.add_argument(
        "--all",
        action="store_true",
        help="every desktop and phone test target, with a total for each",
    )
    parser.add_argument("--fresh", action="store_true", help="take the snapshots again")
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=LISTED,
        help=f"unpaired elements listed on each side, 0 for all (default {LISTED})",
    )


def box(element: Element) -> Box:
    return joined.box(element)


def describe(element: Element) -> str:
    """An element as a reader finds it: its locator, or its role, name and object."""
    key = element.get("key")
    if isinstance(key, str) and key:
        return key
    text = f'{element.get("role")} "{element.get("name") or ""}"'
    thing = element.get("object")
    return text if thing is None else f"{text} object {thing}"


def number(value: float) -> str:
    return f"{value:.1f}".rstrip("0").rstrip(".") if value % 1 else f"{value:.0f}"


def shown(b: Sequence[float]) -> str:
    return "[" + " ".join(number(float(v)) for v in b) + "]"


def signed(value: float) -> str:
    text = number(value)
    return text if text.startswith("-") or text == "0" else f"+{text}"


def lines(element: Element) -> list[Mapping[str, object]]:
    text = element.get("text")
    return list(text) if isinstance(text, list) else []


Line = Mapping[str, object]


def gathered(elements: Sequence[Element], paired: set[int]) -> list[list[Line]]:
    """Each element's lines of text with those of its descendants that are not paired. The two
    clients put a label's text on different elements: the web on a span inside a button, Beryl
    3D on the button itself. A paired descendant's lines are compared with its own pair.
    """
    out: list[list[Line]] = [lines(e) for e in elements]
    # Children come after their parents, so walking backwards finishes a child first.
    for i in range(len(elements) - 1, -1, -1):
        up = joined.parent(elements[i])
        if up is not None and i not in paired:
            out[up] = out[up] + out[i]
    return out


def words(text: Sequence[Line]) -> str:
    return " ".join(" ".join(str(line.get("string", "")).split()) for line in text)


@dataclass(frozen=True)
class Difference:
    """A pair whose boxes or numbers of lines differ."""

    web: Element
    b3d: Element
    # Each side's lines (`gathered`).
    web_lines: list[Line]
    b3d_lines: list[Line]

    def deltas(self) -> Box:
        wx, wy, ww, wh = box(self.web)
        bx, by, bw, bh = box(self.b3d)
        return bx - wx, by - wy, bw - ww, bh - wh

    def worst(self) -> float:
        return max(abs(d) for d in self.deltas())

    def wrap(self) -> str:
        """`WRAP n/m` or `LINES n/m` when the numbers of lines differ, else empty. A pair with
        text on one side only is not counted: a container such as a full-window scrim gathers
        every unpaired line inside it, and text missing from one side is already listed as an
        unmatched element."""
        n, m = len(self.web_lines), len(self.b3d_lines)
        if n == m or not n or not m:
            return ""
        same = words(self.web_lines) == words(self.b3d_lines)
        return f"{'WRAP' if same else 'LINES'} {n}/{m}"

    def line(self) -> str:
        dx, dy, dw, dh = self.deltas()
        text = (
            f"{describe(self.b3d)}  web {shown(box(self.web))}  b3d {shown(box(self.b3d))}  "
            f"dx {signed(dx)} dy {signed(dy)} dw {signed(dw)} dh {signed(dh)}"
        )
        wrap = self.wrap()
        return f"{text}  {wrap}" if wrap else text


def differences(
    web: Sequence[Element],
    b3d: Sequence[Element],
    pairs: Sequence[Mapping[str, object]],
) -> list[Difference]:
    """The pairs whose boxes differ by more than `EXACT` or whose numbers of lines differ,
    worst first: a different number of lines, then the largest difference in points."""
    index = joined.index
    web_lines = gathered(web, {index(p, "web") for p in pairs})
    b3d_lines = gathered(b3d, {index(p, "b3d") for p in pairs})
    found = [
        Difference(
            web[w := index(p, "web")],
            b3d[b := index(p, "b3d")],
            web_lines[w],
            b3d_lines[b],
        )
        for p in pairs
    ]
    found = [d for d in found if d.worst() > EXACT or d.wrap()]
    return sorted(found, key=lambda d: (not d.wrap(), -d.worst()))


def ancestors(elements: Sequence[Element], index: int) -> set[int]:
    out: set[int] = set()
    up = joined.parent(elements[index])
    while up is not None:
        out.add(up)
        up = joined.parent(elements[up])
    return out


def shrunk(b: object) -> Box | None:
    x, y, width, height = joined.as_box(b)
    width, height = width - 2 * SLACK, height - 2 * SLACK
    if width <= 0 or height <= 0:
        return None
    return x + SLACK, y + SLACK, width, height


def intersect(a: Box, b: Box) -> bool:
    return (
        a[0] < b[0] + b[2]
        and b[0] < a[0] + a[2]
        and a[1] < b[1] + b[3]
        and b[1] < a[1] + a[3]
    )


def text_overlaps(
    elements: Sequence[Element], indexes: Sequence[int], text: Sequence[Sequence[Line]]
) -> set[tuple[int, int]]:
    """The pairs of `indexes` (as positions in `indexes`) whose lines of text, `text` by
    element (`gathered`), overlap, leaving out an element and its own ancestor, as a button
    and its label."""
    boxes = [
        [s for line in text[i] if (s := shrunk(line["box"])) is not None]
        for i in indexes
    ]
    families = [ancestors(elements, i) for i in indexes]
    out: set[tuple[int, int]] = set()
    for a in range(len(indexes)):
        for b in range(a + 1, len(indexes)):
            if indexes[a] in families[b] or indexes[b] in families[a]:
                continue
            if any(intersect(p, q) for p in boxes[a] for q in boxes[b]):
                out.add((a, b))
    return out


def one_sided_overlaps(
    web: Sequence[Element],
    b3d: Sequence[Element],
    pairs: Sequence[Mapping[str, object]],
) -> tuple[list[str], list[str]]:
    """The paired elements whose text overlaps in Beryl 3D but not on the web, and those whose
    text overlaps on the web but not in Beryl 3D, as lines naming both elements."""
    index = joined.index
    web_lines = gathered(web, {index(p, "web") for p in pairs})
    b3d_lines = gathered(b3d, {index(p, "b3d") for p in pairs})
    texty: list[Mapping[str, object]] = [
        p for p in pairs if web_lines[index(p, "web")] or b3d_lines[index(p, "b3d")]
    ]
    on_web = text_overlaps(web, [index(p, "web") for p in texty], web_lines)
    in_b3d = text_overlaps(b3d, [index(p, "b3d") for p in texty], b3d_lines)

    def named(found: set[tuple[int, int]]) -> list[str]:
        return [
            f"{describe(b3d[joined.index(texty[a], 'b3d')])}  and  "
            f"{describe(b3d[joined.index(texty[b], 'b3d')])}"
            for a, b in sorted(found)
        ]

    return named(in_b3d - on_web), named(on_web - in_b3d)


@dataclass(frozen=True)
class Found:
    """What `boxes` found for one target."""

    differences: list[Difference]
    unmatched_web: list[Mapping[str, object]]
    unmatched_b3d: list[Mapping[str, object]]
    only_b3d: list[str]
    only_web: list[str]
    passes: dict[str, int]

    def total(self) -> str:
        return (
            f"boxes {len(self.differences)}, unmatched web {len(self.unmatched_web)} "
            f"b3d {len(self.unmatched_b3d)}, overlaps b3d {len(self.only_b3d)} "
            f"web {len(self.only_web)}"
        )


def find(directory: Path) -> Found:
    """Pairs the snapshot in `directory` (writing its `joined.json`) and measures it."""
    web = json.loads((directory / "web.json").read_text())["elements"]
    b3d = json.loads((directory / "b3d.json").read_text())["elements"]
    pairs_and_rest = joined.ensure(directory)
    pairs = pairs_and_rest["pairs"]
    unmatched = pairs_and_rest["unmatched"]
    assert isinstance(pairs, list) and isinstance(unmatched, dict)
    passes: dict[str, int] = {}
    for p in pairs:
        how = str(p["pass"])
        passes[how] = passes.get(how, 0) + 1
    only_b3d, only_web = one_sided_overlaps(web, b3d, pairs)
    return Found(
        differences(web, b3d, pairs),
        unmatched["web"],
        unmatched["b3d"],
        only_b3d,
        only_web,
        passes,
    )


def left_over(
    side: str, listed: Sequence[Mapping[str, object]], limit: int
) -> list[str]:
    out = [f"unmatched on the {side} ({len(listed)}):"]
    shown_now = listed if limit <= 0 else listed[:limit]
    out += [
        f'  {e["role"]} "{e["name"] or ""}" {shown(joined.as_box(e["box"]))}'
        for e in shown_now
    ]
    if len(listed) > len(shown_now):
        out.append(f"  ... {len(listed) - len(shown_now)} more (--limit 0 lists all)")
    return out


def printout(t: Target, found: Found, limit: int) -> str:
    passes = ", ".join(f"{n} by {how}" for how, n in found.passes.items()) or "none"
    out = [
        f"target {t.test_name()} ({t.layout}) at {t.at}: pairs {passes}",
        f"boxes that differ ({len(found.differences)}):",
        *(f"  {d.line()}" for d in found.differences),
        *left_over("web", found.unmatched_web, limit),
        *left_over("Beryl 3D side", found.unmatched_b3d, limit),
        f"text that overlaps only in Beryl 3D ({len(found.only_b3d)}):",
        *(f"  {line}" for line in found.only_b3d),
        f"text that overlaps only on the web ({len(found.only_web)}):",
        *(f"  {line}" for line in found.only_web),
        f"total: {found.total()}",
    ]
    return "\n".join(out) + "\n"


def run_all(args: argparse.Namespace) -> int:
    results = snapshot.every(
        args.recording, args.at, args.fresh, args.art, lambda _, d: find(d).total()
    )
    if results is None:
        return 1
    count = failed = 0
    for t, result in results:
        count += 1
        failed += result is None
        line = result or "failed (the snapshot could not be taken)"
        print(f"{t.test_name()}: {line}", flush=True)
    print(f"{count} targets at {args.at or snapshot.ALL_AT}, {failed} failed")
    return 1 if failed or not count else 0


def run(args: argparse.Namespace) -> int:
    if args.all:
        return run_all(args)
    if not args.recording or not args.at:
        print(
            "match boxes: give a recording and --at, or --all",
            file=sys.stderr,
        )
        return 2
    t = target(args)
    directory = snapshot.ensure(t, fresh=args.fresh, art=args.art)
    if directory is None:
        return 1
    print(printout(t, find(directory), args.limit), end="")
    return 0
