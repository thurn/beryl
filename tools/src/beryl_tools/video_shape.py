"""The shape of a tracked card's motion, beside its timing and size (video_measure.py): how far
its path bows from the straight line between its ends, and how far the card turns and is
squashed on the way. `by video measure` prints these of an Arena clip and `by motion` of a
capture of the stage, as information for the independent motion review (UI Conventions, "Motion
is Beryl's own"; the motion-review skill). Nothing is held to them.

The bow is the largest distance of the card's center from the chord between where it lay and
where it rests, as a share of the chord's length, with the side of the screen it bows toward: a
card thrown in an arc bows up.

Turn and squash are read from the mover's own pixels, where they make a solid card: the mover
in a frame is what differs from both the clip's first and last frames, and a solid rectangle's
second moments give its long and short sides and which way the long one points. A card's turn
is how far its long side has turned from where it points at rest, clockwise on screen; its
squash is its short side over its long one as a multiple of that at rest: 1 as it rests, less
as it turns over about its long side (a face turning in flight passes near 0), more as it tips
away along it. A frame where the mover is not solid (a glow, a card half hidden, one too near
square to say which side is long, a card still over where it lay or where it lands) gives
neither, and a card with no such frame, or none at rest, has none: read those from a
filmstrip."""

from __future__ import annotations

import math
from dataclasses import dataclass

from .video_measure import (
    RUN,
    SOLID,
    THIN,
    THRESHOLD,
    Blob,
    Card,
    Clip,
    _rest,
    blobs,
    differ,
)

# A bow under this share of the chord is a straight path: a pixel or two of the tracker's.
STRAIGHT = 0.02
# A mover whose short side is over this share of its long one is too near square to say which
# way it points.
SQUARE = 0.9


@dataclass(frozen=True)
class Bow:
    """The largest distance of a path from its chord as a share of the chord's length, the
    side of the screen it lies to (`up`, `down`, `left`, `right`; `straight` under STRAIGHT),
    and the time it is reached."""

    share: float
    side: str
    at: float


@dataclass(frozen=True)
class Extreme:
    """A value farthest from a card's rest in its flight, and when."""

    value: float
    at: float


@dataclass(frozen=True)
class Shape:
    """A card's largest turn from how it rests, in degrees clockwise, and its squash farthest
    from 1, over the `solid` of its `frames` that showed a solid card."""

    turn: Extreme
    squash: Extreme
    solid: int
    frames: int


def bow(points: list[tuple[float, float]], times: list[float]) -> Bow | None:
    """The bow of a path through `points` (screen coordinates, y down) at `times`; None for a
    path whose ends are one point."""
    (ax, ay), (bx, by) = points[0], points[-1]
    dx, dy = bx - ax, by - ay
    length = math.hypot(dx, dy)
    if length == 0:
        return None
    far, at, toward = 0.0, times[0], (0.0, 0.0)
    for (x, y), t in zip(points, times):
        along = ((x - ax) * dx + (y - ay) * dy) / (length * length)
        off = (x - ax - along * dx, y - ay - along * dy)
        if math.hypot(*off) > far:
            far, at, toward = math.hypot(*off), t, off
    share = far / length
    if share < STRAIGHT:
        return Bow(round(share, 3), "straight", at)
    if abs(toward[1]) >= abs(toward[0]):
        side = "up" if toward[1] < 0 else "down"
    else:
        side = "left" if toward[0] < 0 else "right"
    return Bow(round(share, 3), side, at)


def origin(clip: Clip, card: Card, threshold: int = THRESHOLD) -> Blob | None:
    """Where `card` lay before it moved: where the clip's first frame differs from its last,
    around where the card was first seen moving. None where nothing its size shows there, as
    for a card that comes off a pile or from outside the crop."""
    left = card.blobs[0]
    found = _rest(clip, differ(clip.frames[-1], clip.frames[0], threshold), left)
    if found is None or not 2 / 3 < found.size / left.size < 3 / 2:
        return None
    return found


def card_bow(clip: Clip, card: Card, threshold: int = THRESHOLD) -> Bow | None:
    """The bow of a tracked card's path: from where it lay, where the clip shows it, through
    its blobs' centers to where it rests."""
    began = origin(clip, card, threshold)
    points = [b.center for b in card.blobs] + [card.rest.center]
    times = card.times + [card.end]
    if began is not None:
        points, times = [began.center] + points, [card.start] + times
    return bow(points, times)


def sides(rows: list[bytes], box: Blob) -> tuple[float, float, float] | None:
    """The long side, the short side and the lean of the long side from upright (degrees
    clockwise on screen, over -90 to 90) of the set pixels of `rows`, the rows of `box`; None
    unless they make a solid shape."""
    n = sx = sy = sxx = syy = sxy = 0.0
    for y, row in enumerate(rows):
        for run in RUN.finditer(row):
            a, b = run.start(), run.end()
            count = b - a
            total = count * (a + b - 1) / 2
            # The squares from a to b - 1, as the difference of two sums from 0.
            squares = ((b - 1) * b * (2 * b - 1) - (a - 1) * a * (2 * a - 1)) / 6
            n += count
            sx += total
            sy += count * y
            sxx += squares
            syy += count * y * y
            sxy += total * y
    if n < 4:
        return None
    cxx = sxx / n - (sx / n) ** 2
    cyy = syy / n - (sy / n) ** 2
    cxy = sxy / n - (sx / n) * (sy / n)
    middle = (cxx + cyy) / 2
    spread = math.hypot((cxx - cyy) / 2, cxy)
    # A solid rectangle's variance along a side of length s is s * s / 12.
    long = math.sqrt(12 * (middle + spread))
    short = math.sqrt(max(0.0, 12 * (middle - spread)))
    if short < THIN or n < SOLID * long * short:
        return None
    lean = math.degrees(math.atan2(2 * cxy, cxx - cyy)) / 2 - 90
    return long, short, _turned(lean)


def _turned(degrees: float) -> float:
    """An angle between two undirected lines, over -90 to 90."""
    return (degrees + 90) % 180 - 90


def mover(clip: Clip, index: int, box: Blob, threshold: int) -> list[bytes]:
    """The rows of `box` in frame `index`, 1 where it differs from both the clip's first and
    last frames by more than `threshold`."""
    first, last, frame = clip.frames[0], clip.frames[-1], clip.frames[index]
    rows: list[bytes] = []
    for y in range(box.y0, box.y1):
        at = slice(y * clip.width + box.x0, y * clip.width + box.x1)
        rows.append(
            bytes(
                abs(v - a) > threshold and abs(v - b) > threshold
                for v, a, b in zip(frame[at], first[at], last[at])
            )
        )
    return rows


def resting(clip: Clip, card: Card, threshold: int) -> tuple[list[bytes], Blob] | None:
    """The card at rest, as rows of its box and the box: where the clip's last frame differs
    from its first, about where the tracker found it at rest. The tracker's own box of it
    reaches only a little past where the card was last seen moving, which cuts a card that
    lands fast, so the whole of what changed there is taken, half a card around."""
    rest = card.rest
    reach = round(rest.size / 2)
    x0, y0 = max(0, rest.x0 - reach), max(0, rest.y0 - reach)
    x1, y1 = min(clip.width, rest.x1 + reach), min(clip.height, rest.y1 + reach)
    first, last = clip.frames[0], clip.frames[-1]
    window = bytearray(clip.width * clip.height)
    for y in range(y0, y1):
        at = slice(y * clip.width + x0, y * clip.width + x1)
        window[at] = bytes(abs(a - b) > threshold for a, b in zip(first[at], last[at]))
    over = [
        b
        for b in blobs(bytes(window), clip.width, clip.height, gap=1)
        if b.joins(rest, 0)
    ]
    if not over:
        return None
    box = max(over, key=lambda b: b.pixels)
    rows = [
        bytes(window[y * clip.width + box.x0 : y * clip.width + box.x1])
        for y in range(box.y0, box.y1)
    ]
    return rows, box


def shape(clip: Clip, card: Card, threshold: int = THRESHOLD) -> Shape | None:
    """How far `card` turns and is squashed in its flight; None where its pixels do not show
    it."""
    lying = resting(clip, card, threshold)
    rest = sides(*lying) if lying is not None else None
    if rest is None or rest[1] > SQUARE * rest[0]:
        return None
    # Over where it lay or where it lands, the mover is only the part of the card that is
    # over neither: a sliver, which says nothing of the card's shape.
    ends = [card.rest, origin(clip, card, threshold) or card.blobs[0]]
    turns: list[Extreme] = []
    squashes: list[Extreme] = []
    for t, blob in zip(card.times, card.blobs):
        if any(blob.joins(end, 0) for end in ends):
            continue
        found = sides(mover(clip, clip.times.index(t), blob, threshold), blob)
        if found is None:
            continue
        long, short, lean = found
        squashes.append(Extreme((short / long) / (rest[1] / rest[0]), t))
        if short <= SQUARE * long:
            turns.append(Extreme(_turned(lean - rest[2]), t))
    if not turns or not squashes:
        return None
    turn = max(turns, key=lambda e: abs(e.value))
    squash = max(squashes, key=lambda e: abs(math.log(e.value)))
    return Shape(
        Extreme(round(turn.value, 1), turn.at),
        Extreme(round(squash.value, 2), squash.at),
        len(squashes),
        len(card.blobs),
    )


def words(found: Bow | None, shaped: Shape | None) -> list[tuple[str, str]]:
    """A card's bow, turn and squash in words: each measure's name and what was found."""
    out: list[tuple[str, str]] = []
    if found is not None:
        side = "straight" if found.side == "straight" else f"bows {found.side}"
        out.append(("bow", f"{found.share:g} of its chord ({side})"))
    if shaped is None:
        return out + [(m, "not shown by its pixels") for m in ("turn", "squash")]
    way = "clockwise" if shaped.turn.value > 0 else "counter-clockwise"
    turn = abs(shaped.turn.value)
    out.append(("turn", f"{turn:g} degrees at most" + (f" ({way})" if turn else "")))
    out.append(
        (
            "squash",
            f"{shaped.squash.value:g} at its farthest from 1"
            f" ({shaped.solid} of {shaped.frames} frames solid)",
        )
    )
    return out


def told(found: Bow | None, shaped: Shape | None) -> dict[str, object]:
    """A card's bow, turn and squash as `by video measure --json` tells them: times in seconds
    on the clip's clock, None for what was not shown."""
    return {
        "bow": found
        and {"share": found.share, "side": found.side, "at": round(found.at, 3)},
        "turn": shaped
        and {"degrees": shaped.turn.value, "at": round(shaped.turn.at, 3)},
        "squash": shaped
        and {"ratio": shaped.squash.value, "at": round(shaped.squash.at, 3)},
    }
