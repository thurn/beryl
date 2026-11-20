"""Motion tracks for `just b3d match motion`: each element's box and opacity in every frame of a
span on both sides, the elements paired across the span, and how far apart each pair's tracks
lie (docs/beryl-3d.md, "Matching the web interface's look").

A side's frames are lists of element records (records.py), taken at the same times. An element
is the same in every frame when its `track` is: on the web motion.mjs gives each DOM node one,
and in Beryl 3D `name_tracks` takes the element id at the end of its locator (`key`), since a
card's locator changes when its face is revealed (`card #259` becomes `card "Snapcaster Mage"
#259`). The matcher (joined.py) pairs each frame
alone, and a Beryl 3D element's partner over the span is the web element it was paired with in
the most frames, so that a frame where a card flies past another of the same role does not swap
the two.
"""

from __future__ import annotations

import math
from collections import Counter
from collections.abc import Mapping, Sequence
from dataclasses import dataclass

from . import joined

Element = Mapping[str, object]
Frame = Sequence[Element]

# The least change, over the span, for an element to count as moving: half a point of its box
# (as `match boxes` counts a box difference) or a hundredth of its opacity.
MOVES_POINTS = 0.5
MOVES_OPACITY = 0.01


@dataclass(frozen=True)
class Sample:
    """An element in one frame: its box in points and the opacity it is seen at."""

    x: float
    y: float
    width: float
    height: float
    opacity: float

    def center(self) -> tuple[float, float]:
        return self.x + self.width / 2, self.y + self.height / 2


# One element's samples, one for each frame, None where it is not on screen.
Track = list[Sample | None]


def sample(element: Element) -> Sample:
    x, y, width, height = joined.box(element)
    style = element.get("style")
    opacity = style.get("opacity", 1) if isinstance(style, Mapping) else 1
    return Sample(x, y, width, height, float(opacity))


def name_tracks(frames: Sequence[Sequence[dict[str, object]]]) -> dict[object, str]:
    """Gives each Beryl 3D record a `track`, the `#<id>` its locator ends with (or the whole
    locator), and returns each track's latest locator, by which it is reported. Records without
    a locator get none."""
    latest: dict[object, str] = {}
    for frame in frames:
        for element in frame:
            key = element.get("key")
            if not isinstance(key, str):
                continue
            head, hash_mark, tail = key.rpartition(" #")
            track = f"#{tail}" if hash_mark and head and tail.isdigit() else key
            element["track"] = track
            latest[track] = key
    return latest


def tracks(frames: Sequence[Frame]) -> dict[object, Track]:
    """Each element's track, by its `track`. Elements without one are left out."""
    out: dict[object, Track] = {}
    for at, frame in enumerate(frames):
        for element in frame:
            name = element.get("track")
            if name is None:
                continue
            if name not in out:
                out[name] = [None for _ in frames]
            out[name][at] = sample(element)
    return out


def partners(web: Sequence[Frame], b3d: Sequence[Frame]) -> dict[object, object]:
    """Each Beryl 3D track's web track over the span: the one the matcher paired it with in the
    most frames, ties going to the earliest. A web track partners one Beryl 3D track at most.
    """
    votes: Counter[tuple[object, object]] = Counter()
    first: dict[tuple[object, object], int] = {}
    for at, (web_frame, b3d_frame) in enumerate(zip(web, b3d)):
        pairs = joined.join(web_frame, b3d_frame)["pairs"]
        assert isinstance(pairs, list)
        for pair in pairs:
            key = b3d_frame[joined.index(pair, "b3d")].get("track")
            track = web_frame[joined.index(pair, "web")].get("track")
            if key is None or track is None:
                continue
            votes[(key, track)] += 1
            first.setdefault((key, track), at)
    out: dict[object, object] = {}
    taken: set[object] = set()
    for (key, track), _ in sorted(
        votes.items(), key=lambda item: (-item[1], first[item[0]])
    ):
        if key not in out and track not in taken:
            out[key] = track
            taken.add(track)
    return out


def inside(frames: Sequence[Frame], outer: set[object]) -> set[object]:
    """The tracks of the elements that lie inside an element whose track is in `outer` (its
    `parent` chain, an index into the same frame, reaches one) in some frame. A card's text
    and frame on the web are elements of their own inside the card, which move with it.
    """
    found: set[object] = set()
    for frame in frames:
        for element in frame:
            parent = element.get("parent")
            seen = 0
            while (
                isinstance(parent, int)
                and 0 <= parent < len(frame)
                and seen < len(frame)
            ):
                if frame[parent].get("track") in outer:
                    found.add(element.get("track"))
                    break
                parent = frame[parent].get("parent")
                seen += 1
    return found


def moves(track: Track) -> bool:
    """Whether the element's box or opacity changes over the span, or it comes or goes."""
    seen = [s for s in track if s is not None]
    if not seen:
        return False
    if len(seen) != len(track):
        return True
    start = seen[0]
    return any(
        max(
            abs(s.x - start.x),
            abs(s.y - start.y),
            abs(s.width - start.width),
            abs(s.height - start.height),
        )
        > MOVES_POINTS
        or abs(s.opacity - start.opacity) > MOVES_OPACITY
        for s in seen
    )


@dataclass(frozen=True)
class Worst:
    """The largest difference of one kind between two tracks and the frame it is in."""

    amount: float
    frame: int


@dataclass(frozen=True)
class Difference:
    """How far apart a pair's tracks lie, over the frames where both elements are on screen:
    the distance between their boxes' centers, the larger difference of width or height, and
    the difference of opacity. `shared` counts those frames; None in a field means there are
    none."""

    position: Worst | None
    size: Worst | None
    opacity: Worst | None
    shared: int


def worst(values: Sequence[tuple[int, float]]) -> Worst | None:
    if not values:
        return None
    frame, amount = max(values, key=lambda value: value[1])
    return Worst(amount, frame)


def difference(web: Track, b3d: Track) -> Difference:
    both = [
        (at, w, b)
        for at, (w, b) in enumerate(zip(web, b3d))
        if w is not None and b is not None
    ]
    return Difference(
        position=worst([(at, math.dist(w.center(), b.center())) for at, w, b in both]),
        size=worst(
            [
                (at, max(abs(w.width - b.width), abs(w.height - b.height)))
                for at, w, b in both
            ]
        ),
        opacity=worst([(at, abs(w.opacity - b.opacity)) for at, w, b in both]),
        shared=len(both),
    )


def present(track: Track) -> str:
    """The frames an element is on screen in, as `0-30` or `4-30` or `0-9, 14-30`."""
    runs: list[str] = []
    start: int | None = None
    for at, s in enumerate([*track, None]):
        if s is not None and start is None:
            start = at
        elif s is None and start is not None:
            runs.append(f"{start}-{at - 1}" if at - 1 > start else str(start))
            start = None
    return ", ".join(runs) or "none"


# How `match motion --all` weighs each kind of difference into one number, so that a verb's
# pairs can be summed and the verbs ranked. Each weight turns its kind into points of the
# boxes' positions: a point of size counts as a point of position, a hundredth of opacity as
# a point, and a 60 Hz frame (about 16.7 ms) of timing as a point and two thirds, so that a
# motion which starts one frame late counts about as much as one that is a point and a half
# off its path for the whole span.
WEIGHTS: dict[str, float] = {
    "position": 1.0,
    "size": 1.0,
    "opacity": 100.0,
    "timing": 0.1,
}


def moving_frames(track: Track) -> tuple[int, int] | None:
    """The first and last frame in which the element is moving: on screen and away from where
    it stood in the frame before, or coming or going. None if it never moves."""
    changed: list[int] = []
    for at in range(1, len(track)):
        before, now = track[at - 1], track[at]
        if (before is None) != (now is None):
            changed.append(at)
        elif before is not None and now is not None and moves([before, now]):
            changed.append(at)
    return (changed[0], changed[-1]) if changed else None


@dataclass(frozen=True)
class Deviation:
    """How far apart a pair's tracks lie on average, for ranking: the mean distance between the
    boxes' centers, the mean larger difference of width or height and the mean difference of
    opacity over the frames both elements are on screen, and how many ms apart the two sides'
    motions start plus how many apart they end (0 unless both sides move)."""

    position: float
    size: float
    opacity: float
    timing: float

    def weighed(self) -> dict[str, float]:
        """Each kind's share of `total`, by `WEIGHTS`."""
        return {kind: getattr(self, kind) * WEIGHTS[kind] for kind in WEIGHTS}

    def total(self) -> float:
        return sum(self.weighed().values())

    def most(self) -> str:
        """The kind that adds the most to `total`."""
        shares = self.weighed()
        return max(shares, key=lambda kind: shares[kind])


def deviation(web: Track, b3d: Track, times: Sequence[float]) -> Deviation:
    """`Deviation` of two tracks taken at `times` (ms, one for each frame)."""
    both = [(w, b) for w, b in zip(web, b3d) if w is not None and b is not None]
    count = max(1, len(both))
    timing = 0.0
    web_moving, b3d_moving = moving_frames(web), moving_frames(b3d)
    if web_moving is not None and b3d_moving is not None:
        timing = abs(times[web_moving[0]] - times[b3d_moving[0]]) + abs(
            times[web_moving[1]] - times[b3d_moving[1]]
        )
    return Deviation(
        position=sum(math.dist(w.center(), b.center()) for w, b in both) / count,
        size=sum(max(abs(w.width - b.width), abs(w.height - b.height)) for w, b in both)
        / count,
        opacity=sum(abs(w.opacity - b.opacity) for w, b in both) / count,
        timing=timing,
    )


def alone(track: Track) -> Deviation:
    """The `Deviation` of an element that moves on one side only, against standing still: how far
    its center, its size and its opacity get from where they first are on screen."""
    seen = [s for s in track if s is not None]
    if not seen:
        return Deviation(0.0, 0.0, 0.0, 0.0)
    first = seen[0]
    return Deviation(
        position=max(math.dist(s.center(), first.center()) for s in seen),
        size=max(
            max(abs(s.width - first.width), abs(s.height - first.height)) for s in seen
        ),
        opacity=max(abs(s.opacity - first.opacity) for s in seen),
        timing=0.0,
    )
