"""`just b3d match census`: where on the screen Beryl 3D differs most from the web interface,
over many moments of many recordings, ranked by screen area (docs/beryl-3d.md, "Matching the
web interface's look").

`score` measures each reference target once, at its recording's end, so a difference that
shows only mid-game (a combat step, a tall stack, a prompt with a chooser) never appears in it,
and one large region such as the cloth outweighs the rest of a target. The census measures a
fixed list of moments instead and gives every difference it finds to one screen area.

- **The moments.** `data/beryl3d/match/census.json` lists, for each recording, the moments the
  census measures, in milliseconds. They are up to `PER_RECORDING` of the interface tests'
  check times (the moments at which the client's checks run, which `just b3d info --json`
  lists as `check_times_ms`), spread across the recording. The recordings are every built
  screen of `data/beryl3d/screens.json` and every UI fixture played by two agents from a seed
  (`ui/fixtures.json`). `--choose` chooses the moments again and rewrites the file, so that
  every other run measures the same moments.
- **The areas.** `AREAS` is the table of screen areas, each with the Beryl 3D source it lives
  in. A Beryl 3D element belongs to the first area that it, or failing that its nearest
  ancestor, matches (`b3d_areas`). A web element belongs to the area of its nearest paired
  ancestor, itself included (`web_areas`).
- **The counts.** Each moment is taken as `snapshot` takes it, in the desktop window at scale 2
  from the seat `you`, and measured as `boxes`, `styles` and `pixels` measure it. Each
  differing pixel (`b3d-render pixels`) goes to the area of the topmost Beryl 3D element that
  paints there, else to the web element's, else to the cloth, so the areas' pixels add up to
  `score`'s count. Each box and style difference and unsupported style value goes to its
  pair's Beryl 3D element's area, and each unpaired element to its own area.

The report lists each area with its totals and the three moments where it differs most in
pixels, each with the `just b3d match pixels` command that shows it. It is written to
`$BERYL_CACHE/b3d/match/census/<digest>/` as `census.json` and `census.txt`. Each moment's
counts are kept beside its snapshot and reused while the snapshot's inputs are unchanged, so a
second run with no change only reads files.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from ... import paths as repo_paths
from .. import compare
from .. import paths as beryl3d_paths
from . import boxes, joined, reference, score, snapshot, styles
from .target import Target, target

Element = Mapping[str, Any]

MOMENTS: Path = beryl3d_paths.DATA / "match" / "census.json"
FIXTURES: Path = repo_paths.UI / "fixtures.json"
# The window, seat and scale every moment is measured in: the desktop app's on a Retina Mac.
WINDOW = "1600x900@2"
SEAT = "you"
# The most moments `--choose` takes from one recording.
PER_RECORDING = 8
# The most browser pages the census opens at once, so that it leaves the machine's shared page
# slots to other sessions' checks.
PAGES = 2
# The counts each area holds, in the order the report lists them.
COUNTS: tuple[str, ...] = ("pixels", "boxes", "styles", "unsupported", "unmatched")
# How many moments the report names for each area.
WORST = 3
# Where Beryl 3D's scene code lives, which each area's source is under.
SCENE = "rust/beryl3d/crates/beryl_scene/src"


@dataclass(frozen=True)
class Area:
    """One screen area: a Beryl 3D element belongs to it when its role is in `roles`, its name
    matches `names` (a whole-name pattern), its place on the table (the record's `place`, as
    `battlefield.yours.lands`) starts with one of `places`, or its parent's role is in
    `inside`."""

    name: str
    # The file or directory of Beryl 3D's source that draws the area.
    source: str
    roles: frozenset[str] = frozenset()
    names: re.Pattern[str] | None = None
    places: tuple[str, ...] = ()
    inside: frozenset[str] = frozenset()

    def matches(self, element: Element, parent: Element | None) -> bool:
        if element.get("role") in self.roles:
            return True
        name = str(element.get("name") or "")
        if self.names is not None and self.names.fullmatch(name):
            return True
        place = str(element.get("place") or "")
        if place and any(place.startswith(p) for p in self.places):
            return True
        return parent is not None and parent.get("role") in self.inside


def region_names(*names: str) -> re.Pattern[str]:
    return re.compile("|".join(re.escape(n) for n in names))


# The screen areas, in the order an element is tried against them: the first that matches wins.
# A card's printed parts come before the zones, so that a mana cost drawn on a card in hand is
# part of the card face and the card itself part of the hand.
AREAS: tuple[Area, ...] = (
    Area(
        "card face",
        f"{SCENE}/faces.rs",
        inside=frozenset({"card", "ability"}),
    ),
    Area("stack", f"{SCENE}/table/stack.rs", places=("stack",)),
    Area("hand", f"{SCENE}/table/hands.rs", places=("hand.",)),
    Area("battlefield", f"{SCENE}/table/rows", places=("battlefield.",)),
    Area(
        "piles",
        f"{SCENE}/table/pile.rs",
        roles=frozenset({"pile"}),
        names=re.compile(r"(Your|Opponent's) (library|graveyard|exile)( \d+)?"),
        places=("library.", "graveyard.", "exile.", "sideboard.", "revealed"),
    ),
    Area("prompt", f"{SCENE}/prompts", names=region_names("prompt")),
    Area("game log", f"{SCENE}/hud/log", names=region_names("log", "Game log")),
    Area(
        "life orbs",
        f"{SCENE}/hud/players.rs",
        roles=frozenset({"player"}),
        names=region_names("players", "your orb", "opponent's orb"),
    ),
    Area(
        "steps",
        f"{SCENE}/hud/steps.rs",
        names=region_names("steps", "Your steps", "Opponent's steps"),
    ),
    Area(
        "screens",
        f"{SCENE}/screens",
        names=region_names(
            "start",
            "settings",
            "bug report",
            "gallery",
            "testing",
            "lobby",
            "table",
            "corner",
        ),
    ),
    # The game's flat layer: a pixel of it that no nearer area claims is the table's cloth.
    Area("cloth", f"{SCENE}/table/scene.rs", names=region_names("game")),
)
# The area of an element no entry of `AREAS` claims.
OTHER = "other"
# The area of a differing pixel that no element on either side paints, and of a web element
# with no paired ancestor whose box covers at least `BACKGROUND` of the window: both are the
# table's cloth showing through.
CLOTH = "cloth"
BACKGROUND = 0.5
NAMES: tuple[str, ...] = (*(a.name for a in AREAS), OTHER)
SOURCES: dict[str, str] = {a.name: a.source for a in AREAS}


def census_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--areas",
        default=None,
        help=f"report only these areas, comma-separated ({', '.join(NAMES)})",
    )
    parser.add_argument(
        "--recordings",
        default="",
        metavar="PREFIX",
        help="measure only the recordings whose names start with this",
    )
    parser.add_argument("--json", action="store_true", help="print the JSON report")
    parser.add_argument("--fresh", action="store_true", help="take the snapshots again")
    parser.add_argument(
        "--choose",
        action="store_true",
        help=f"choose the moments again from the check times and rewrite {MOMENTS.name}",
    )


def b3d_areas(elements: Sequence[Element]) -> list[str]:
    """Each Beryl 3D element's area: the first of `AREAS` that it or its nearest ancestor
    matches, or `OTHER`. Parents come before their children, so each element reads its
    parent's area once that is known."""
    out: list[str] = []
    for element in elements:
        up = joined.parent(element)
        parent = None if up is None else elements[up]
        own = next((a.name for a in AREAS if a.matches(element, parent)), None)
        out.append(own or (OTHER if up is None else out[up]))
    return out


def web_areas(
    web: Sequence[Element],
    b3d_area: Sequence[str],
    pairs: Sequence[Mapping[str, object]],
    window: float,
) -> list[str]:
    """Each web element's area: that of the Beryl 3D pair of it or its nearest paired
    ancestor. An element with no paired ancestor is the cloth when its own box covers at least
    `BACKGROUND` of the `window`'s area, in square points, and `OTHER` otherwise: the page's
    root wrappers cover the window, and what they hold unpaired is not the cloth."""
    paired = {joined.index(p, "web"): joined.index(p, "b3d") for p in pairs}
    out: list[str] = []
    # Whether each element or one of its ancestors is paired.
    anchored: list[bool] = []
    for i, element in enumerate(web):
        up = joined.parent(element)
        if i in paired:
            out.append(b3d_area[paired[i]])
            anchored.append(True)
        elif up is not None and anchored[up]:
            out.append(out[up])
            anchored.append(True)
        else:
            _, _, width, height = joined.box(element)
            out.append(CLOTH if width * height >= BACKGROUND * window else OTHER)
            anchored.append(False)
    return out


def tally(
    web: Sequence[Element],
    b3d: Sequence[Element],
    pixels: Mapping[str, Any] | None,
    window: float,
) -> dict[str, dict[str, int]]:
    """One moment's counts by area, from both sides' records and `b3d-render pixels`'s
    `pixels.json` (None to count no pixels)."""
    found = joined.join(web, b3d)
    pairs = found["pairs"]
    unmatched = found["unmatched"]
    assert isinstance(pairs, list) and isinstance(unmatched, dict)
    on_b3d = b3d_areas(b3d)
    on_web = web_areas(web, on_b3d, pairs, window)
    out: dict[str, dict[str, int]] = {}

    def add(area: str, count: str, n: int = 1) -> None:
        if n:
            held = out.setdefault(area, {c: 0 for c in COUNTS})
            held[count] += n

    # A box difference holds its elements, not their indexes.
    b3d_index = {id(e): i for i, e in enumerate(b3d)}
    for d in boxes.differences(web, b3d, pairs):
        add(on_b3d[b3d_index[id(d.b3d)]], "boxes")
    for p in pairs:
        b = joined.index(p, "b3d")
        differing, unsupported = styles.compare_pair(
            web[joined.index(p, "web")], b3d[b]
        )
        add(on_b3d[b], "styles", len(differing))
        add(on_b3d[b], "unsupported", len(unsupported))
    for side, areas in (("web", on_web), ("b3d", on_b3d)):
        for left in unmatched[side]:
            add(areas[int(left["index"])], "unmatched")
    if pixels is not None:
        # A differing pixel is in two rows when its topmost elements on the two sides are not
        # each other's pairs: its web element's (`web_pixels`) and its Beryl 3D element's
        # (`b3d_pixels`). So that each pixel counts once, it goes to its Beryl 3D element's
        # area, else (`b3d_bare`, where no Beryl 3D element paints) to its web element's, and
        # a pixel neither side paints to the cloth.
        for row in pixels["rows"]:
            if row.get("b3d") is not None:
                add(on_b3d[int(row["b3d"])], "pixels", int(row["b3d_pixels"]))
            if row.get("web") is not None:
                add(on_web[int(row["web"])], "pixels", int(row["b3d_bare"]))
        add(CLOTH, "pixels", int(pixels.get("nobody", 0)))
    return out


def window_area(t: Target) -> float:
    width, height = t.width_height()
    return float(width * height)


# The file beside a snapshot that keeps its census counts, with the snapshot's inputs.
KEPT = "census.json"


def measure(t: Target, directory: Path) -> dict[str, dict[str, int]] | None:
    """The counts by area of `t`'s snapshot in `directory`, read from its kept file when the
    snapshot has not changed since; None when `b3d-render pixels` failed."""
    inputs = (directory / "inputs.txt").read_text()
    kept = directory / KEPT
    if kept.is_file():
        held = json.loads(kept.read_text())
        if held.get("inputs") == inputs:
            areas: dict[str, dict[str, int]] = held["areas"]
            return areas
    web = json.loads((directory / "web.json").read_text())["elements"]
    b3d = json.loads((directory / "b3d.json").read_text())["elements"]
    joined.ensure(directory)
    if score.pixel_count(t, directory) is None:
        return None
    pixels = json.loads((directory / "pixels.json").read_text())
    areas = tally(web, b3d, pixels, window_area(t))
    kept.write_text(json.dumps({"inputs": inputs, "areas": areas}, indent=1) + "\n")
    return areas


def moment_target(recording: str, ms: int) -> Target:
    return target(
        argparse.Namespace(
            recording=f"b3d:{recording}/{SEAT}/{WINDOW}",
            at=f"{ms}ms",
            seat=None,
            layout=None,
            size=None,
            scale=None,
        )
    )


def spread(times: Sequence[int], most: int) -> list[int]:
    """At most `most` of `times`, spread across them: for each of `most` equal spans of the
    recording up to its last time, the time nearest the span's middle, each taken once.
    """
    unique = sorted(set(times))
    if len(unique) <= most:
        return unique
    end = unique[-1]
    chosen: list[int] = []
    for k in range(most):
        middle = end * (k + 0.5) / most
        left = [t for t in unique if t not in chosen]
        chosen.append(min(left, key=lambda t: (abs(t - middle), t)))
    return sorted(chosen)


def recordings() -> list[str]:
    """The recordings `--choose` takes moments from: every built screen that can be drawn
    (`reference.UNDRAWABLE`), then every UI fixture that two agents play from a seed."""
    screens = json.loads(reference.SCREENS.read_text())["screens"]
    names = [
        s["recording"]
        for s in screens
        if s.get("built") and s["recording"] not in reference.UNDRAWABLE
    ]
    fixtures = json.loads(FIXTURES.read_text())["fixtures"]
    names += [f["name"] for f in fixtures if "agents" in f and "seed" in f]
    return list(dict.fromkeys(names))


def choose(prefix: str) -> int:
    """Rewrites the moments file with up to `PER_RECORDING` check times of each recording
    whose name starts with `prefix`, keeping the other recordings' moments."""
    t = moment_target("x", 0)
    held: dict[str, list[int]] = {}
    if MOMENTS.is_file():
        held = load_moments()
    for name in recordings():
        if not name.startswith(prefix):
            continue
        printed = compare.b3d(["info", name, *t.common(), "--json"])
        if printed is None:
            print(f"match census: `b3d info {name}` failed", file=sys.stderr)
            return 1
        times = json.loads(printed.strip().splitlines()[-1])["check_times_ms"]
        held[name] = spread([int(ms) for ms in times], PER_RECORDING)
        print(f"{name}: {len(held[name])} of {len(set(times))} check times")
    about = (
        "The moments `just b3d match census` measures, in milliseconds, by recording: up "
        f"to {PER_RECORDING} of each recording's check times, spread across it, from the "
        f"seat {SEAT} in the window {WINDOW}. `just b3d match census --choose` rewrites it."
    )
    # One recording to a line, so that choosing one recording's moments again is a short diff.
    lines = ",\n".join(f"  {json.dumps(n)}: {json.dumps(ms)}" for n, ms in held.items())
    text = f'{{\n "about": {json.dumps(about)},\n "recordings": {{\n{lines}\n }}\n}}\n'
    MOMENTS.write_text(text)
    print(f"wrote {MOMENTS}: {sum(len(ms) for ms in held.values())} moments")
    return 0


def load_moments() -> dict[str, list[int]]:
    moments: dict[str, list[int]] = json.loads(MOMENTS.read_text())["recordings"]
    return moments


@dataclass
class Total:
    """One area's totals over the moments, and its worst moments by pixels."""

    counts: dict[str, int] = field(default_factory=lambda: dict.fromkeys(COUNTS, 0))
    # (pixels, recording, moment in ms) of each moment the area differs in.
    moments: list[tuple[int, str, int]] = field(default_factory=list)

    def worst(self) -> list[tuple[int, str, int]]:
        return sorted(self.moments, key=lambda m: (-m[0], m[1], m[2]))[:WORST]


def pixels_command(recording: str, ms: int) -> str:
    return f"just b3d match pixels {recording} --at {ms}ms"


def totals(
    measured: Mapping[tuple[str, int], Mapping[str, Mapping[str, int]]],
) -> dict[str, Total]:
    """Each area's totals over the `measured` moments (by recording and moment)."""
    out: dict[str, Total] = {}
    for (recording, ms), areas in sorted(measured.items()):
        for name, counts in areas.items():
            total = out.setdefault(name, Total())
            for count in COUNTS:
                total.counts[count] += int(counts.get(count, 0))
            if counts.get("pixels"):
                total.moments.append((int(counts["pixels"]), recording, ms))
    return out


def ranked(found: Mapping[str, Total], chosen: Sequence[str]) -> list[str]:
    """The `chosen` areas that differ anywhere, most differing pixels first."""
    return sorted(
        (name for name in found if name in chosen),
        key=lambda name: (
            -found[name].counts["pixels"],
            -sum(found[name].counts.values()),
            name,
        ),
    )


def report_json(
    command: str,
    measured: Mapping[tuple[str, int], Mapping[str, Mapping[str, int]]],
    failed: Sequence[str],
    chosen: Sequence[str],
) -> dict[str, Any]:
    found = totals(measured)
    return {
        "command": command,
        "window": WINDOW,
        "seat": SEAT,
        "moments": len(measured),
        "failed": list(failed),
        "areas": [
            {
                "area": name,
                "source": SOURCES.get(name),
                **found[name].counts,
                "worst": [
                    {
                        "recording": recording,
                        "at": f"{ms}ms",
                        "pixels": pixels,
                        "command": pixels_command(recording, ms),
                    }
                    for pixels, recording, ms in found[name].worst()
                ],
            }
            for name in ranked(found, chosen)
        ],
        "by_moment": [
            {"recording": recording, "at": f"{ms}ms", "areas": areas}
            for (recording, ms), areas in sorted(measured.items())
        ],
    }


def report_text(document: Mapping[str, Any]) -> str:
    rows = document["areas"]
    width = max([len("area"), *(len(row["area"]) for row in rows)])
    head = f"{'area':<{width}}" + "".join(f"  {c:>11}" for c in COUNTS) + "  source"
    out = [
        document["command"],
        f"{document['moments']} moments in {document['window']} from the seat "
        f"{document['seat']}, by screen area, most differing pixels first",
        head,
    ]
    for row in rows:
        counts = "".join(f"  {row[c]:>11}" for c in COUNTS)
        out.append(f"{row['area']:<{width}}{counts}  {row['source'] or '-'}")
    for row in rows:
        if row["worst"]:
            out.append(f"{row['area']}, worst moments:")
            out += [f"  {w['pixels']:>9}  {w['command']}" for w in row["worst"]]
    if document["failed"]:
        out.append(f"failed (no snapshot or pixels): {', '.join(document['failed'])}")
    return "\n".join(out) + "\n"


def written(document: Mapping[str, Any], digests: Sequence[str]) -> Path:
    """Writes the report under `$BERYL_CACHE/b3d/match/census/<digest>/`, the digest being
    that of the moments' snapshot inputs and the areas reported; returns the directory.
    """
    digest = hashlib.sha256()
    for part in (*sorted(digests), json.dumps(document["command"])):
        digest.update(part.encode())
        digest.update(b"\0")
    out = beryl3d_paths.cache() / "match" / "census" / digest.hexdigest()[:16]
    out.mkdir(parents=True, exist_ok=True)
    (out / "census.json").write_text(json.dumps(document, indent=1) + "\n")
    (out / "census.txt").write_text(report_text(document))
    return out


def command_line(args: argparse.Namespace) -> str:
    parts = ["just b3d match census"]
    if args.areas:
        parts += ["--areas", args.areas]
    if args.recordings:
        parts += ["--recordings", args.recordings]
    return " ".join(parts)


def run(args: argparse.Namespace) -> int:
    if args.choose:
        return choose(args.recordings)
    chosen = list(NAMES)
    if args.areas:
        chosen = [a.strip() for a in args.areas.split(",") if a.strip()]
        unknown = [a for a in chosen if a not in NAMES]
        if unknown:
            print(
                f"match census: no area {', '.join(unknown)}; the areas are "
                f"{', '.join(NAMES)}",
                file=sys.stderr,
            )
            return 2
    moments = {
        (name, ms): moment_target(name, ms)
        for name, times in load_moments().items()
        if name.startswith(args.recordings)
        for ms in times
    }
    if not moments:
        print(
            f"match census: no recording of {MOMENTS.name} starts with "
            f"{args.recordings!r}",
            file=sys.stderr,
        )
        return 1
    by_target = {t: key for key, t in moments.items()}
    measured: dict[tuple[str, int], dict[str, dict[str, int]]] = {}
    failed: list[str] = []
    digests: list[str] = []
    results = snapshot.measured(
        list(moments.values()), args.fresh, False, measure, pages=PAGES
    )
    for t, areas in results:
        recording, ms = by_target[t]
        if areas is None:
            failed.append(f"{recording} at {ms}ms")
            continue
        measured[(recording, ms)] = areas
        digests.append((t.directory() / "inputs.txt").read_text())
    document = report_json(command_line(args), measured, sorted(failed), chosen)
    out = written(document, digests)
    if args.json:
        print(json.dumps(document, indent=1))
    else:
        print(report_text(document), end="")
        print(f"wrote {out / 'census.json'} and census.txt")
    return 1 if failed or not measured else 0
