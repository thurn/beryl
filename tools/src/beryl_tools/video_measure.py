"""`by video measure`: the timing, path and size of a card's motion, measured the same way in an
Arena recording and in a folder of Beryl frames (plan K27, docs/project-50k.md).

A clip is a span of a recording, cropped and scaled down, or a folder of frames whose names
start with their time in milliseconds (ui/scripts/film.mjs writes `00120-yours.jpg`). ffmpeg
decodes either into gray frames; the rest is plain Python.

The moving card is told apart from the table by comparing each frame with the clip's first and
last frames: a pixel is the mover's when it differs from both. That drops the table, the place
the card left (it matches the last frame) and the place it lands (it matches the first), so the
first two frames and the last two must show the card at rest. The mover's pixels are grouped
into blobs, and blobs are followed from frame to frame into tracks, one per card, trimmed of
the light frames at either end (a card entering at the crop's edge, a glow where it landed), but
not those of a card small and whole there, as one growing or shrinking on its way. A
card's motion runs from the last frame before its track to the first frame after it; its size
at rest is where the last frame differs from the first, around the place it landed; its peak
is the largest of its boxes in flight, as a multiple of that.

Arena's table is animated and busy. When the clip shows a different number of movers from the
cards asked for, or a card is moving at either end, the command says so and exits non-zero
with no numbers: crop closer, move the span, or read that motion from a filmstrip instead.
Measurements are in seconds on the clip's clock; positions are fractions of the whole frame
(the table), so Arena's 1920 by 1080 and Beryl's 1600 by 900 compare."""

from __future__ import annotations

import argparse
import json
import math
import re
import subprocess
import sys
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

from . import paths, video
from .video import FFMPEG, VideoError

FFPROBE = "ffprobe"
# Gray levels a pixel must change by to count: above compression noise and the table's
# shimmer, below a card's edge. Arena's cast and draw clips isolated their card at 35, and
# at neither 20 nor 50 (by-w8gv.21.17).
THRESHOLD = 35
# Pixels apart that still join one blob, so a card speckled where it matches the table holds.
GAP = 3
# A blob smaller than this share of the clip's pixels is noise.
SPECK = 0.0005
# A track whose largest blob is under this share of the largest track's is the table moving.
MINOR = 0.15
# A blob changed over this share of its box is solid: a card, not a glow.
SOLID = 0.6
# A blob thinner than this, in working pixels, is an edge of a card, never a whole one.
THIN = 3
# The default width the crop is scaled to: enough for a card's size to 2%, cheap in Python.
WORK_WIDTH = 480
# Path samples the table prints for each card; JSON has every frame.
SAMPLES = 8
IMAGES = (".png", ".jpg", ".jpeg")
RUN: re.Pattern[bytes] = re.compile(rb"\x01+")
PTS: re.Pattern[bytes] = re.compile(rb"Parsed_showinfo.*?pts_time:\s*(-?[0-9.]+)")


class Unmeasurable(Exception):
    """The clip does not isolate the cards asked for; the message says what to change, and
    `moved` holds the boxes of each thing seen moving, for the overlay."""

    def __init__(self, message: str, moved: list[list[Blob]] | None = None) -> None:
        super().__init__(message)
        self.moved: list[list[Blob]] = moved or []


@dataclass(frozen=True)
class Clip:
    """Gray frames, `width` by `height` bytes each, and where they sit on the table: working
    pixel (x, y) is source pixel (left + x * scale, top + y * scale) of a `table` sized frame.
    """

    width: int
    height: int
    times: list[float]
    frames: list[bytes]
    table: tuple[int, int]
    left: float = 0.0
    top: float = 0.0
    scale: float = 1.0


@dataclass(frozen=True)
class Blob:
    """A box in working pixels, end-exclusive, and how many of its pixels changed."""

    x0: int
    y0: int
    x1: int
    y1: int
    pixels: int

    @property
    def center(self) -> tuple[float, float]:
        return ((self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2)

    @property
    def size(self) -> float:
        """The side of a square of the box's area: a card's size whichever way it turns."""
        return math.sqrt((self.x1 - self.x0) * (self.y1 - self.y0))

    def joins(self, other: Blob, gap: int) -> bool:
        return (
            self.x0 - gap <= other.x1
            and other.x0 <= self.x1 + gap
            and self.y0 - gap <= other.y1
            and other.y0 <= self.y1 + gap
        )

    def union(self, other: Blob) -> Blob:
        return Blob(
            min(self.x0, other.x0),
            min(self.y0, other.y0),
            max(self.x1, other.x1),
            max(self.y1, other.y1),
            self.pixels + other.pixels,
        )


@dataclass(frozen=True)
class Card:
    """One card's motion: the frames it was seen moving in, their blobs, and its blob at
    rest. `start` and `end` are the times of the still frames either side."""

    start: float
    end: float
    times: list[float]
    blobs: list[Blob]
    rest: Blob

    @property
    def peak(self) -> tuple[float, float]:
        """The largest size in flight as a multiple of the size at rest, and when."""
        size, t = max((b.size, t) for b, t in zip(self.blobs, self.times))
        return size / self.rest.size, t


def differ(a: bytes, b: bytes, threshold: int) -> bytes:
    """1 where the frames differ by more than `threshold`, else 0."""
    return bytes([abs(x - y) > threshold for x, y in zip(a, b)])


def differ_both(frame: bytes, first: bytes, last: bytes, threshold: int) -> bytes:
    """1 where `frame` differs from both `first` and `last`: the mover, not where it was or
    will be."""
    return bytes(
        [
            abs(x - a) > threshold and abs(x - b) > threshold
            for x, a, b in zip(frame, first, last)
        ]
    )


def blobs(mask: bytes, width: int, height: int, gap: int = GAP) -> list[Blob]:
    """The groups of set pixels, joining those within `gap` of each other, then any whose
    boxes touch. Rows are scanned as runs, so the work is in C but for the joins."""
    parent: list[int] = []
    boxes: list[Blob] = []

    def root(n: int) -> int:
        while parent[n] != n:
            parent[n] = parent[parent[n]]
            n = parent[n]
        return n

    recent: list[tuple[int, int, int, int]] = []
    for y in range(height):
        row = mask[y * width : (y + 1) * width]
        runs: list[list[int]] = []
        for match in RUN.finditer(row):
            if runs and match.start() - runs[-1][1] <= gap:
                runs[-1][1] = match.end()
            else:
                runs.append([match.start(), match.end()])
        recent = [r for r in recent if y - r[2] <= gap + 1]
        fresh: list[tuple[int, int, int, int]] = []
        for x0, x1 in runs:
            n = len(parent)
            parent.append(n)
            boxes.append(Blob(x0, y, x1, y + 1, row.count(1, x0, x1)))
            for a0, a1, _, other in recent:
                if a0 - gap <= x1 and x0 <= a1 + gap:
                    parent[root(other)] = root(n)
            fresh.append((x0, x1, y, n))
        recent += fresh
    groups: dict[int, Blob] = {}
    for n, box in enumerate(boxes):
        r = root(n)
        groups[r] = groups[r].union(box) if r in groups else box
    found = list(groups.values())
    merged = True
    while merged:
        merged = False
        joined: list[Blob] = []
        for blob in found:
            for k, other in enumerate(joined):
                if other.joins(blob, 0):
                    joined[k] = other.union(blob)
                    merged = True
                    break
            else:
                joined.append(blob)
        found = joined
    return found


def track(frames: list[list[Blob]]) -> list[list[tuple[int, Blob]]]:
    """Follows blobs from frame to frame: each joins the track whose predicted center is
    nearest, within a card's diagonal and a half, and starts a track of its own otherwise. A
    track may miss one frame."""
    tracks: list[list[tuple[int, Blob]]] = []
    for i, found in enumerate(frames):
        live = [t for t in tracks if t[-1][0] >= i - 2]
        pairs: list[tuple[float, int, int]] = []
        for k, t in enumerate(live):
            j, last = t[-1]
            x, y = last.center
            if len(t) > 1:
                h, before = t[-2]
                step = (i - j) / (j - h)
                x += (x - before.center[0]) * step
                y += (y - before.center[1]) * step
            for b, blob in enumerate(found):
                reach = 1.5 * max(_diagonal(last), _diagonal(blob))
                d = math.dist((x, y), blob.center)
                if d <= reach:
                    pairs.append((d, k, b))
        taken_tracks: set[int] = set()
        taken_blobs: set[int] = set()
        for _, k, b in sorted(pairs):
            if k in taken_tracks or b in taken_blobs:
                continue
            live[k].append((i, found[b]))
            taken_tracks.add(k)
            taken_blobs.add(b)
        tracks += [[(i, blob)] for b, blob in enumerate(found) if b not in taken_blobs]
    return tracks


def _diagonal(blob: Blob) -> float:
    return math.hypot(blob.x1 - blob.x0, blob.y1 - blob.y0)


def measure(
    clip: Clip,
    cards: int = 1,
    threshold: int = THRESHOLD,
    landing: tuple[float, float] | None = None,
) -> list[Card]:
    """Each of the `cards` movers in the clip, in the order they start moving. With `landing`,
    a point in working pixels, only movers last seen over it count: a capture that knows where
    its card comes to rest leaves out what moves around it, such as a row making room.
    """
    if len(clip.frames) < 3:
        raise Unmeasurable(f"{len(clip.frames)} frames: a clip needs at least 3")
    first, last = clip.frames[0], clip.frames[-1]
    speck: int = max(4, round(SPECK * clip.width * clip.height))

    def found(mask: bytes) -> list[Blob]:
        return [b for b in blobs(mask, clip.width, clip.height) if b.pixels >= speck]

    seen = [found(differ_both(f, first, last, threshold)) for f in clip.frames]
    tracks = [_body(clip, t) for t in track(seen) if len(t) >= 2]
    if landing is not None:
        x, y = landing
        tracks = [
            t
            for t in tracks
            if t[-1][1].x0 <= x <= t[-1][1].x1 and t[-1][1].y0 <= y <= t[-1][1].y1
        ]
    if not tracks:
        raise Unmeasurable("nothing moved: check the span and the crop")
    largest = max(b.pixels for t in tracks for _, b in t)
    tracks = [t for t in tracks if max(b.pixels for _, b in t) >= MINOR * largest]
    moved = [[b for _, b in t] for t in tracks]
    if len(tracks) != cards:
        raise Unmeasurable(
            f"{len(tracks)} {'thing' if len(tracks) == 1 else 'things'} moved where "
            f"{cards} {'card was' if cards == 1 else 'cards were'} asked for: crop closer, "
            "shorten the span, or pass --cards",
            moved,
        )
    tracks.sort(key=lambda t: t[0][0])
    change = differ(last, first, threshold)
    measured: list[Card] = []
    for n, t in enumerate(tracks, 1):
        begin, end = t[0][0], t[-1][0]
        # The first and last frames are the references, so a card moving between them and
        # their neighbors would look still: two still frames are needed at either end.
        if begin <= 1:
            raise Unmeasurable(
                f"card {n} is moving in the first frame: start earlier", moved
            )
        if end >= len(clip.frames) - 2:
            raise Unmeasurable(
                f"card {n} is moving in the last frame: end later", moved
            )
        landed = t[-1][1]
        rest = _rest(clip, change, landed)
        if rest is None:
            raise Unmeasurable(f"card {n} is not at rest in the last frame", moved)
        if not 2 / 3 < rest.size / landed.size < 3 / 2:
            raise Unmeasurable(
                f"card {n} at rest is {rest.size / landed.size:.2f} times its size when "
                "it landed: something else changed there, or its shadow or glow moved "
                "with it",
                moved,
            )
        measured.append(
            Card(
                start=clip.times[begin - 1],
                end=clip.times[end + 1],
                times=[clip.times[i] for i, _ in t],
                blobs=[b for _, b in t],
                rest=rest,
            )
        )
    return measured


def _body(clip: Clip, t: list[tuple[int, Blob]]) -> list[tuple[int, Blob]]:
    """A track without its light ends. A card entering at the crop's edge, a glow lingering
    where it landed, or an edge of the card left as it settles onto its rest leaves a track
    with little of the card in it at either end. A card that is small there, growing as it
    leaves or shrinking as it lands, is whole: solid and inside the crop."""
    heaviest = max(b.pixels for _, b in t)
    heavy = [
        k
        for k, (_, b) in enumerate(t)
        if b.pixels >= MINOR * heaviest or _whole(clip, b)
    ]
    return t[heavy[0] : heavy[-1] + 1]


def _whole(clip: Clip, blob: Blob) -> bool:
    """Whether a blob is a whole card rather than a piece of one or a glow: clear of the
    crop's edges, no mere line, and changed over most of its box."""
    inside = (
        blob.x0 > 0 and blob.y0 > 0 and blob.x1 < clip.width and blob.y1 < clip.height
    )
    thick = min(blob.x1 - blob.x0, blob.y1 - blob.y0) >= THIN
    area = (blob.x1 - blob.x0) * (blob.y1 - blob.y0)
    return inside and thick and blob.pixels >= SOLID * area


def _rest(clip: Clip, change: bytes, landed: Blob) -> Blob | None:
    """The card at rest: where the last frame differs from the first, near where the card
    was last seen moving. Only that neighborhood is searched, so other changes on the table
    cannot join it, and only pieces centered on the landing count, so a card lying a pixel
    or two away, as in a hand, does not either."""
    margin = round(landed.size / 8)
    x0, y0 = max(0, landed.x0 - margin), max(0, landed.y0 - margin)
    x1, y1 = min(clip.width, landed.x1 + margin), min(clip.height, landed.y1 + margin)
    window = bytearray(len(change))
    for y in range(y0, y1):
        row = slice(y * clip.width + x0, y * clip.width + x1)
        window[row] = change[row]
    parts = [
        b
        for b in blobs(bytes(window), clip.width, clip.height, gap=1)
        if b.pixels >= 4
        and landed.x0 <= b.center[0] <= landed.x1
        and landed.y0 <= b.center[1] <= landed.y1
    ]
    if not parts:
        return None
    rest = parts[0]
    for part in parts[1:]:
        rest = rest.union(part)
    return rest


def on_table(clip: Clip, x: float, y: float) -> tuple[float, float]:
    """A working pixel as fractions of the table's width and height."""
    w, h = clip.table
    return ((clip.left + x * clip.scale) / w, (clip.top + y * clip.scale) / h)


def report(clip: Clip, cards: list[Card], since: float) -> dict[str, object]:
    """The measurements as JSON: seconds on the clip's clock, milliseconds for spans."""

    def ms(seconds: float) -> int:
        return round(seconds * 1000)

    out: list[dict[str, object]] = []
    for card in cards:
        peak, peak_at = card.peak
        rest_w = (card.rest.x1 - card.rest.x0) * clip.scale / clip.table[0]
        rest_h = (card.rest.y1 - card.rest.y0) * clip.scale / clip.table[1]
        out.append(
            {
                "start": round(card.start, 3),
                "end": round(card.end, 3),
                "duration_ms": ms(card.end - card.start),
                "delay_ms": ms(card.start - since),
                "peak": round(peak, 3),
                "peak_at": round(peak_at, 3),
                "rest": [round(rest_w, 4), round(rest_h, 4)],
                "path": [
                    {
                        "t": round(t, 3),
                        "x": round(on_table(clip, *b.center)[0], 4),
                        "y": round(on_table(clip, *b.center)[1], 4),
                        "size": round(b.size / card.rest.size, 3),
                    }
                    for t, b in zip(card.times, card.blobs)
                ],
            }
        )
    starts = [card.start for card in cards]
    return {
        "table": list(clip.table),
        "since": round(since, 3),
        "cards": out,
        "stagger_ms": [ms(b - a) for a, b in zip(starts, starts[1:])],
    }


def table_text(found: dict[str, object], at: Callable[[float], str]) -> str:
    """The report as a table, times shown by `at`."""
    cards = found["cards"]
    assert isinstance(cards, list)
    lines = [
        f"{'card':<5}{'start':>12}{'delay':>9}{'duration':>10}{'peak':>8}"
        f"{'at':>12}  rest (w x h of table)"
    ]
    for n, card in enumerate(cards, 1):
        rest = card["rest"]
        lines.append(
            f"{n:<5}{at(card['start']):>12}{card['delay_ms']:>7}ms{card['duration_ms']:>8}ms"
            f"{card['peak']:>7.2f}x{at(card['peak_at']):>12}  {rest[0]:.3f} x {rest[1]:.3f}"
        )
    stagger = found["stagger_ms"]
    assert isinstance(stagger, list)
    if stagger:
        lines.append("stagger: " + ", ".join(f"{s}ms" for s in stagger))
    for n, card in enumerate(cards, 1):
        path = card["path"]
        lines.append(
            f"path of card {n}: center as a fraction of the table, size at rest 1"
        )
        picks = sorted(
            {round(i * (len(path) - 1) / (SAMPLES - 1)) for i in range(SAMPLES)}
        )
        for i in picks:
            p = path[i]
            lines.append(
                f"  {at(p['t']):>12}  {p['x']:.3f} {p['y']:.3f}  {p['size']:.2f}x"
            )
    return "\n".join(lines)


# Decoding: ffmpeg turns a recording's span or a folder's frames into gray bytes.


def frame_size(source: Path) -> tuple[int, int]:
    run = subprocess.run(
        [FFPROBE, "-v", "error", "-select_streams", "v:0"]
        + ["-show_entries", "stream=width,height", "-of", "csv=p=0", str(source)],
        capture_output=True,
        text=True,
        check=False,
    )
    if run.returncode != 0:
        raise VideoError(f"ffprobe failed on {source}:\n{run.stderr}")
    w, h = run.stdout.strip().splitlines()[0].split(",")[:2]
    return int(w), int(h)


def geometry(
    table: tuple[int, int], crop: str | None, width: int
) -> tuple[list[int], int, int, str]:
    """The crop in source pixels, the working size and the filter that makes it."""
    x, y, w, h = (int(p) for p in crop.split(",")) if crop else (0, 0, *table)
    work_w = min(width, w)
    work_h = max(1, round(h * work_w / w))
    filters = f"crop={w}:{h}:{x}:{y},scale={work_w}:{work_h}:flags=area,format=gray"
    return [x, y, w, h], work_w, work_h, filters


def decode(args: list[str], size: int) -> tuple[bytes, bytes]:
    run = subprocess.run(
        [FFMPEG, "-hide_banner", "-nostats", *args],
        capture_output=True,
        check=False,
    )
    if run.returncode != 0:
        raise VideoError(f"ffmpeg failed:\n{run.stderr.decode(errors='replace')}")
    if not run.stdout or len(run.stdout) % size:
        raise VideoError("ffmpeg gave no whole frames: check the time and the crop")
    return run.stdout, run.stderr


def read_recording(
    source: Path, start: float, span: float, crop: str | None, width: int
) -> Clip:
    """A recording's frames from `start` for `span` seconds, at its own rate."""
    table = frame_size(source)
    box, w, h, filters = geometry(table, crop, width)
    raw, log = decode(
        ["-ss", f"{start:.3f}", "-t", f"{span:.3f}", "-i", str(source)]
        + ["-vf", f"{filters},showinfo", "-fps_mode", "passthrough"]
        + ["-f", "rawvideo", "-"],
        w * h,
    )
    frames = [raw[i : i + w * h] for i in range(0, len(raw), w * h)]
    times = [start + float(t) for t in PTS.findall(log)]
    if len(times) != len(frames):
        raise VideoError(f"{len(frames)} frames but {len(times)} times from ffmpeg")
    return Clip(w, h, times, frames, table, box[0], box[1], box[2] / w)


def frame_files(folder: Path, pattern: str) -> list[tuple[float, Path]]:
    """The frames in a folder whose names start with their time in ms, as (seconds, path)."""
    found: dict[float, Path] = {}
    for path in sorted(folder.glob(pattern)):
        match = re.match(r"(\d+(?:\.\d+)?)", path.name)
        if path.suffix.lower() not in IMAGES or not match:
            continue
        t = float(match.group(1)) / 1000
        if t in found:
            raise VideoError(
                f"{found[t].name} and {path.name} share a time: pass --glob to pick one seat"
            )
        found[t] = path
    return sorted(found.items())


def read_folder(
    folder: Path,
    pattern: str,
    crop: str | None,
    width: int,
    window: tuple[float, float] | None,
) -> Clip:
    """A folder's frames, within `window` (seconds) if given."""
    files = frame_files(folder, pattern)
    if window:
        files = [(t, p) for t, p in files if window[0] <= t <= window[1]]
    if not files:
        raise VideoError(f"no frames named by their time in ms in {folder}")
    table = frame_size(files[0][1])
    box, w, h, filters = geometry(table, crop, width)
    size: int = w * h
    vf: str = filters

    def one(path: Path) -> bytes:
        args = ["-v", "error", "-i", str(path), "-vf", vf, "-frames:v", "1"]
        return decode(args + ["-f", "rawvideo", "-"], size)[0][:size]

    with ThreadPoolExecutor(max_workers=8) as pool:
        frames = list(pool.map(one, [p for _, p in files]))
    return Clip(w, h, [t for t, _ in files], frames, table, box[0], box[1], box[2] / w)


def overlay(clip: Clip, moved: list[tuple[list[Blob], Blob | None]], name: str) -> Path:
    """Draws each mover's boxes in flight, its path and its box at rest over the first frame,
    into the cache, for a person to check what was tracked."""
    w: int = clip.width
    h: int = clip.height
    rgb: bytearray = bytearray(
        b for v in clip.frames[0] for b in (v // 2, v // 2, v // 2)
    )
    colors = [(255, 80, 80), (80, 200, 255), (120, 255, 120), (255, 200, 60)]

    def dot(x: int, y: int, color: tuple[int, int, int]) -> None:
        if 0 <= x < w and 0 <= y < h:
            rgb[(y * w + x) * 3 : (y * w + x) * 3 + 3] = bytes(color)

    def outline(b: Blob, color: tuple[int, int, int]) -> None:
        for x in range(b.x0, b.x1):
            dot(x, b.y0, color)
            dot(x, b.y1 - 1, color)
        for y in range(b.y0, b.y1):
            dot(b.x0, y, color)
            dot(b.x1 - 1, y, color)

    for n, (boxes, rest) in enumerate(moved):
        color = colors[n % len(colors)]
        dim = (color[0] // 2, color[1] // 2, color[2] // 2)
        for b in boxes:
            outline(b, dim)
        if rest is not None:
            outline(rest, (255, 255, 255))
        centers = [b.center for b in boxes]
        for (x0, y0), (x1, y1) in zip(centers, centers[1:]):
            steps = max(1, int(max(abs(x1 - x0), abs(y1 - y0))))
            for s in range(steps + 1):
                dot(
                    round(x0 + (x1 - x0) * s / steps),
                    round(y0 + (y1 - y0) * s / steps),
                    color,
                )
    out = paths.cache_root() / "video" / "measure" / f"{name}.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    run = subprocess.run(
        [FFMPEG, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"]
        + ["-s", f"{w}x{h}", "-i", "-", str(out)],
        input=bytes(rgb),
        capture_output=True,
        check=False,
    )
    if run.returncode != 0:
        raise VideoError(f"ffmpeg failed:\n{run.stderr.decode(errors='replace')}")
    return out


def command(args: argparse.Namespace) -> int:
    """Measures, prints the table (or JSON) and the overlay's path; exits 1 with a reason and
    no numbers when the clip does not isolate the cards."""
    source = Path(args.source).expanduser()
    try:
        if source.is_dir():
            window = None
            if args.at:
                begin = float(args.at) / 1000
                window = (begin, begin + float(args.span))
            clip = read_folder(source, args.glob, args.crop, int(args.width), window)
            since = float(args.since) / 1000 if args.since else 0.0

            def at(t: float) -> str:
                return f"{t * 1000:.0f}ms"

            name = f"{source.name}-{source.parent.name}"
        else:
            if not args.at:
                raise VideoError("a recording needs the time to start at")
            recording, _ = video.resolve(args.source)
            start = video.parse_clock(args.at)
            clip = read_recording(
                recording, start, float(args.span), args.crop, int(args.width)
            )
            since = video.parse_clock(args.since) if args.since else start

            def at(t: float) -> str:
                return video.clock(t, 3)

            name = f"{video.video_id(recording)}-{video.shot_name(start)[:-4]}"
        try:
            cards = measure(clip, int(args.cards), int(args.threshold))
        except Unmeasurable as error:
            print(f"cannot measure: {error}", file=sys.stderr)
            if error.moved:
                drawn = overlay(clip, [(boxes, None) for boxes in error.moved], name)
                print(f"what moved: {drawn}", file=sys.stderr)
            return 1
        image = overlay(clip, [(card.blobs, card.rest) for card in cards], name)
    except (VideoError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    found = report(clip, cards, since)
    found["overlay"] = str(image)
    if args.json:
        print(json.dumps(found, indent=2))
    else:
        print(table_text(found, at))
        print(f"overlay: {image}")
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = commands.add_parser(
        "measure", help="time, path and size of a moving card in a clip"
    )
    p.add_argument("source", help="a recording or its id, or a folder of frames")
    p.add_argument(
        "at",
        nargs="?",
        help="where to start: 24:32.5 in a recording; ms in a folder (default all)",
    )
    p.add_argument("--span", default="2", help="seconds to cover (default 2)")
    p.add_argument("--crop", help="x,y,w,h in the source's pixels")
    p.add_argument(
        "--since",
        help="the instant the delay counts from, on the clip's clock "
        "(default the start; 0 ms in a folder)",
    )
    p.add_argument(
        "--cards", default="1", help="cards moving, as in a deal (default 1)"
    )
    p.add_argument(
        "--glob", default="*", help="a folder's frames to read, as *-yours.jpg"
    )
    p.add_argument(
        "--width", default=str(WORK_WIDTH), help=f"working width (default {WORK_WIDTH})"
    )
    p.add_argument(
        "--threshold",
        default=str(THRESHOLD),
        help=f"gray levels that count as change (default {THRESHOLD})",
    )
    p.add_argument("--json", action="store_true", help="print JSON instead")
    p.set_defaults(func=command)
