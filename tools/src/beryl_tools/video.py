"""`by video`: a searchable index of an Arena gameplay recording, as a reference for the UI.

The indexes live outside the tree in ~/arena ($BERYL_ARENA), a folder per recording named by
its id; the recordings are there or in ~/Movies/arena, and `list` shows them. `index` reads
the text on screen twice a second (ffmpeg decodes, Apple's Vision framework recognizes:
video_ocr.swift) and writes, into ~/arena/<id>/:

- `text.jsonl`: every sampled frame's text with its position, in 1920x1080 pixels.
- `events.jsonl` and `timeline.txt`: what that text says happened. Arena shows the top of the
  stack as a large card at the right edge, a spell with its type line and an ability with the
  type line "Ability", so a name appearing there is a cast or an ability. Prompts are the
  centered headings, with the buttons offered beside them. Life totals, turn banners, each new
  opponent and the draft are events too.
- `shots/`: one frame per event.

`find` searches both layers. `film` tiles the frames around a moment into one image, at any
rate and crop, to study an animation without stepping through the video.

Nothing here knows the rules: an event says what was on screen, not who controls it, and what
never reaches the stack or a heading (a land drop, combat damage) is only in the text layer.
The regions are Arena's desktop layout at 16:9. A recording of another shape shows that table
letterboxed (a 16:10 screen) or pillarboxed, so every frame is cut to the table first: shots,
filmstrips and their crops are all in its pixels. Only macOS has the recognizer."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
from collections import Counter
from collections.abc import Callable, Iterable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass, field
from pathlib import Path

from . import paths

# Where the indexes are written, each recording's in a folder named by its id.
ARENA_ENV = "BERYL_ARENA"
ARENA_DEFAULT: Path = Path.home() / "arena"
# Where recordings are looked for by id, after that folder.
MOVIES: Path = Path.home() / "Movies" / "arena"
RECORDINGS = (".webm", ".mp4", ".mkv", ".mov")
FFMPEG = "ffmpeg"
OCR_SOURCE: Path = Path(__file__).with_name("video_ocr.swift")
# Positions are in these pixels whatever the recording's size.
WIDTH = 1920.0
HEIGHT = 1080.0
# Cuts a frame to the centered 16:9 table: a 1920x1200 recording has bars of 60 above and below.
TABLE = "crop='min(iw,ih*16/9)':'min(ih,iw*9/16)'"
# Two readings name the same thing when they are this alike: recognition drops and swaps
# letters while a card flies in or a glow passes over it.
ALIKE = 0.7
# Samples a reading may be missing for without ending its run.
GAP = 2


@dataclass(frozen=True)
class Line:
    """A line of text on screen: its top left corner and size."""

    text: str
    x: float
    y: float
    w: float
    h: float

    @property
    def center(self) -> float:
        return self.x + self.w / 2


@dataclass(frozen=True)
class Frame:
    """A sampled frame's text, `t` seconds into the recording."""

    t: float
    lines: list[Line]


@dataclass
class Event:
    t: float
    end: float
    kind: str
    name: str
    detail: str = ""
    buttons: list[str] = field(default_factory=list)
    shot: str = ""


@dataclass(frozen=True)
class Box:
    """A region a line starts in, and the text heights it accepts."""

    x0: float
    x1: float
    y0: float
    y1: float
    h0: float = 0.0
    h1: float = HEIGHT
    w1: float = WIDTH

    def has(self, line: Line) -> bool:
        return (
            self.x0 <= line.x <= self.x1
            and self.y0 <= line.y <= self.y1
            and self.h0 <= line.h <= self.h1
            and line.w <= self.w1
        )

    def first(self, frame: Frame) -> str:
        return next((line.text for line in frame.lines if self.has(line)), "")

    def all(self, frame: Frame) -> list[str]:
        found = [line for line in frame.lines if self.has(line)]
        return [line.text for line in sorted(found, key=lambda line: line.y)]


# The card shown for the top of the stack: its name, type line and rules text.
STACK_NAME = Box(1555, 1612, 272, 302, h0=16)
STACK_TYPE = Box(1565, 1612, 515, 550)
STACK_TEXT = Box(1570, 1640, 555, 705)
# The buttons at the bottom right: "Next", "Resolve", "No Blocks", "Submit 1".
BUTTONS = Box(1560, 1900, 880, 1010, h0=18)
# A browser's heading ("Surveil", "Choose One") and the instructions under it.
HEADING = Box(300, 1500, 40, 120, h0=38)
INSTRUCTIONS = Box(300, 1500, 115, 200, h0=18, h1=38)
# The one-line prompt over the battlefield ("Choose attackers.", "Discard a card.").
PROMPT = Box(300, 1500, 425, 470, h0=24, h1=55)
# "YOUR TURN" across the battlefield.
BANNER = Box(300, 1500, 380, 520, h0=60)
# How far a heading's middle may be from the screen's.
CENTERED = 70.0
# A life total. Mana in the pool beside it reads as more digits, in a wider, taller line.
LIFE_THEIRS = Box(900, 1000, 128, 178, h0=24, h1=45, w1=70)
LIFE_YOURS = Box(900, 1000, 865, 930, h0=24, h1=45, w1=70)
OPPONENT = Box(55, 140, 15, 48, h0=20)
# The draft's button, on every pick, read loosely ("Confirm Rick", "nfirm Pick"), but its last
# word must have three letters: a game's "Confirm 0" is as alike.
DRAFT = "Confirm Pick"


def alike(a: str, b: str) -> bool:
    """Whether two readings are of the same text: nearly equal, or one a part of the other."""
    a, b = a.lower(), b.lower()
    if min(len(a), len(b)) >= 4 and (a in b or b in a):
        return True
    return difflib.SequenceMatcher(None, a, b).ratio() >= ALIKE


def clean_name(text: str) -> str:
    """A card's name without what its mana cost reads as: the symbols come out as a short
    last word of digits, capitals and marks ("Sheoldred, the Apocalypse 29C")."""
    words = text.split()
    while len(words) > 1 and len(words[-1]) <= 4 and not re.search(r"[a-z]", words[-1]):
        words.pop()
    return " ".join(words)


def best(readings: Iterable[str]) -> str:
    """The reading seen most, the longest of equals."""
    counts = Counter(reading for reading in readings if reading)
    return max(counts, key=lambda r: (counts[r], len(r)), default="")


@dataclass
class Run:
    """Consecutive samples reading the same text: the samples' indices and readings."""

    indices: list[int]
    readings: list[str]

    @property
    def text(self) -> str:
        return best(self.readings)


def runs(readings: list[str]) -> list[Run]:
    """Groups each sample's reading ("" for none) into runs of the same text."""
    found: list[Run] = []
    for index, reading in enumerate(readings):
        if not reading:
            continue
        last = found[-1] if found else None
        if (
            last is not None
            and index - last.indices[-1] <= GAP + 1
            and alike(reading, last.text)
        ):
            last.indices.append(index)
            last.readings.append(reading)
        else:
            found.append(Run([index], [reading]))
    return found


def centered(box: Box, frame: Frame) -> str:
    for line in frame.lines:
        if box.has(line) and abs(line.center - WIDTH / 2) <= CENTERED:
            return line.text
    return ""


def is_draft(frame: Frame) -> bool:
    return any(
        alike(line.text, DRAFT) and sum(c.isalpha() for c in line.text.split()[-1]) >= 3
        for line in frame.lines
        if len(line.text) > 8
    )


def run_events(
    frames: list[Frame],
    read: Callable[[Frame], str],
    make: Callable[[Run], Event | None],
    least: int = 1,
) -> list[Event]:
    """An event per run of `read`'s text lasting `least` samples, as `make` describes it."""
    events: list[Event] = []
    for run in runs([read(frame) for frame in frames]):
        if len(run.indices) < least:
            continue
        event = make(run)
        if event is not None:
            events.append(event)
    return events


def buttons(frames: list[Frame], run: Run) -> list[str]:
    """The buttons offered during a run, those read at least twice."""
    counts = Counter(
        text
        for index in range(run.indices[0], run.indices[-1] + 1)
        for text in BUTTONS.all(frames[index])
    )
    labels: list[str] = []
    for text, count in counts.most_common():
        if count >= 2 and not any(alike(text, label) for label in labels):
            labels.append(text)
    return sorted(labels)


def stack_kind(frame: Frame) -> str:
    """Whether the stack's card is a spell or an ability, by its type line; "" when the line
    is not readable yet."""
    type_line = STACK_TYPE.first(frame)
    if not type_line:
        return ""
    return "ability" if alike(type_line, "Ability") else "spell"


def by_kind(frames: list[Frame], run: Run) -> list[Run]:
    """Splits a run of one name where its card turns from a spell into an ability or back: a
    permanent spell resolves and its own trigger takes its place. A kind read once is
    noise."""
    kinds = [stack_kind(frames[index]) for index in run.indices]
    parts: list[tuple[str, Run]] = []
    for n, kind in enumerate(kinds):
        if kind and (not parts or parts[-1][0] != kind):
            if kinds[n : n + 2] == [kind, kind] or not parts:
                parts.append((kind, Run([], [])))
        if parts:
            parts[-1][1].indices.append(run.indices[n])
            parts[-1][1].readings.append(run.readings[n])
    if not parts:
        return [run]
    # Samples before the first readable type line belong to it.
    head = kinds.index(parts[0][0])
    parts[0][1].indices[:0] = run.indices[:head]
    parts[0][1].readings[:0] = run.readings[:head]
    return [part for _, part in parts]


def stack_events(frames: list[Frame]) -> list[Event]:
    """Each spell or ability as it becomes the top of the stack. While the stack stays up, a
    name seen before is the object under a resolved one showing again, not a new one."""
    events: list[Event] = []
    seen: list[tuple[bool, str]] = []
    last = -GAP - 2
    named = runs([clean_name(STACK_NAME.first(frame)) for frame in frames])
    for run in (part for whole in named for part in by_kind(frames, whole)):
        if run.indices[0] - last > GAP + 1:
            seen = []
        last = run.indices[-1]
        shown = [frames[index] for index in run.indices]
        ability = best(stack_kind(frame) for frame in shown) == "ability"
        if any(was == ability and alike(run.text, name) for was, name in seen):
            continue
        seen.append((ability, run.text))
        type_line = best(STACK_TYPE.first(frame) for frame in shown)
        text = best(" ".join(STACK_TEXT.all(frame)) for frame in shown)
        events.append(
            Event(
                t=frames[run.indices[0]].t,
                end=frames[run.indices[-1]].t,
                kind="ability" if ability else "spell",
                name=run.text,
                detail=text if ability else type_line,
                buttons=buttons(frames, run),
            )
        )
    return events


def prompt_events(frames: list[Frame]) -> list[Event]:
    def instructions(frame: Frame) -> str:
        return " ".join(text for text in INSTRUCTIONS.all(frame) if not text.isdigit())

    def prompt(run: Run, detail: str = "") -> Event:
        t, end = frames[run.indices[0]].t, frames[run.indices[-1]].t
        found = buttons(frames, run)
        return Event(
            t=t, end=end, kind="prompt", name=run.text, detail=detail, buttons=found
        )

    def heading(run: Run) -> Event:
        return prompt(run, best(instructions(frames[index]) for index in run.indices))

    def banner(run: Run) -> Event | None:
        if not run.text.upper().endswith("TURN"):
            return None
        t = frames[run.indices[0]].t
        return Event(t=t, end=frames[run.indices[-1]].t, kind="turn", name=run.text)

    return [
        *run_events(frames, lambda f: centered(HEADING, f), heading, least=2),
        *run_events(frames, lambda f: centered(PROMPT, f), prompt, least=2),
        *run_events(frames, lambda f: centered(BANNER, f), banner),
    ]


def life_events(frames: list[Frame]) -> list[Event]:
    """Each change of a life total, once two samples in a row agree on it."""
    events: list[Event] = []
    for who, box in (("you", LIFE_YOURS), ("opponent", LIFE_THEIRS)):
        current = ""
        previous = ""
        for frame in frames:
            reading = box.first(frame)
            if reading.isdigit() and reading == previous and reading != current:
                if current:
                    name = f"{who} {current} to {reading}"
                    events.append(Event(t=frame.t, end=frame.t, kind="life", name=name))
                current = reading
            previous = reading
    return events


def opponent_events(frames: list[Frame]) -> list[Event]:
    """Each new opponent: the name at the top left, read for three seconds."""

    def name(frame: Frame) -> str:
        # The avatar's frame reads as a stray first letter: "y Karl", ") Emali".
        words = OPPONENT.first(frame).split()
        return " ".join(words[1:] if len(words) > 1 and len(words[0]) == 1 else words)

    events: list[Event] = []
    for run in runs([name(frame) for frame in frames]):
        if len(run.indices) >= 6 and not (events and alike(events[-1].name, run.text)):
            t = frames[run.indices[0]].t
            events.append(Event(t=t, end=t, kind="match", name=run.text))
    return events


def derive(frames: list[Frame]) -> list[Event]:
    """The events the frames' text shows, in order. Draft frames carry no game."""
    drafting = [is_draft(frame) for frame in frames]
    game = [
        Frame(frame.t, [] if draft else frame.lines)
        for frame, draft in zip(frames, drafting)
    ]
    events = [
        *stack_events(game),
        *prompt_events(game),
        *life_events(game),
        *opponent_events(game),
    ]
    started = -math.inf
    for frame, draft in zip(frames, drafting):
        if draft:
            if frame.t - started > 30:
                events.append(Event(t=frame.t, end=frame.t, kind="draft", name="draft"))
            started = frame.t
    return sorted(events, key=lambda event: event.t)


def clock(seconds: float, places: int = 1) -> str:
    """`24:32.5`, or `1:02:03.0` past the hour."""
    minutes, rest = divmod(round(seconds, places), 60)
    hours, minutes = divmod(int(minutes), 60)
    tail = f"{rest:0{places + 3}.{places}f}"
    return f"{hours}:{minutes:02}:{tail}" if hours else f"{minutes}:{tail}"


def parse_clock(text: str) -> float:
    """Seconds from `1472`, `24:32` or `1:02:03.5`."""
    seconds = 0.0
    for part in text.split(":"):
        seconds = seconds * 60 + float(part)
    return seconds


def timeline(events: list[Event]) -> str:
    lines: list[str] = []
    for event in events:
        line = f"{clock(event.t):>9}  {event.kind:<8} {event.name}"
        if event.detail:
            line += f" | {event.detail}"
        if event.buttons:
            line += f" [{', '.join(event.buttons)}]"
        lines.append(line)
    return "\n".join(lines) + "\n"


class VideoError(Exception):
    """Indexing failed; the message says why."""


def video_id(source: Path) -> str:
    """The YouTube id yt-dlp leaves in brackets, else the file's name, as a folder name."""
    match = re.search(r"\[([\w-]{6,})\]$", source.stem)
    name = match.group(1) if match else re.sub(r"[^\w-]+", "-", source.stem)[:48]
    return name.strip("-") or "video"


def root() -> Path:
    """The folder of indexes: $BERYL_ARENA, else ~/arena."""
    folder = os.environ.get(ARENA_ENV)
    return Path(folder).expanduser() if folder else ARENA_DEFAULT


def recordings(folder: Path) -> dict[str, Path]:
    """The recordings in a folder, by id."""
    files = sorted(folder.iterdir()) if folder.is_dir() else []
    return {video_id(f): f for f in files if f.suffix.lower() in RECORDINGS}


def known() -> dict[str, Path]:
    """The recordings in the index folder and in ~/Movies/arena, by id."""
    return {**recordings(MOVIES), **recordings(root())}


def resolve(name: str) -> tuple[Path, Path]:
    """A recording and its index folder, from the recording's path or its id."""
    path = Path(name).expanduser()
    if path.is_file():
        return path.resolve(), root() / video_id(path)
    found = known().get(name)
    if found is not None:
        return found, root() / name
    meta = root() / name / "meta.json"
    if not meta.is_file():
        raise VideoError(
            f"{name}: neither a file nor a recording's id (`by video list`)"
        )
    return Path(str(json.loads(meta.read_text())["source"])), meta.parent


def recognizer() -> Path:
    """The compiled recognizer, built into the cache when its source changes."""
    if sys.platform != "darwin":
        raise VideoError("`by video index` needs macOS: it reads text with Vision")
    digest = hashlib.sha256(OCR_SOURCE.read_bytes()).hexdigest()[:12]
    binary = paths.cache_root() / "video" / f"ocr-{digest}"
    if not binary.exists():
        binary.parent.mkdir(parents=True, exist_ok=True)
        run = subprocess.run(
            ["swiftc", "-O", str(OCR_SOURCE), "-o", str(binary)],
            capture_output=True,
            text=True,
            check=False,
        )
        if run.returncode != 0:
            raise VideoError(f"swiftc failed:\n{run.stderr}")
    return binary


def table_box(width: int, height: int) -> tuple[int, int, int, int]:
    """x, y, w and h of the table in a frame of this size, as `TABLE` cuts it."""
    w, h = min(width, height * 16 // 9), min(height, width * 9 // 16)
    return (width - w) // 2, (height - h) // 2, w, h


def ffmpeg(*args: str) -> None:
    run = subprocess.run(
        [FFMPEG, "-v", "error", "-y", *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if run.returncode != 0:
        raise VideoError(f"ffmpeg failed:\n{run.stderr}")


def recognize(images: list[Path], jobs: int) -> dict[Path, list[Line]]:
    """The text in each image. Vision works one image at a time in a process, so the images
    are dealt to `jobs` of them."""
    binary: Path = recognizer()

    def work(share: list[Path]) -> str:
        listing = "".join(f"{path}\n" for path in share)
        return subprocess.run(
            [str(binary)], input=listing, capture_output=True, text=True, check=True
        ).stdout

    shares = [images[n::jobs] for n in range(jobs)]
    found: dict[Path, list[Line]] = {}
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        for output in pool.map(work, shares):
            for record in map(json.loads, output.splitlines()):
                found[Path(record["path"])] = [
                    Line(
                        text=str(line["text"]),
                        x=round(float(line["x"]) * WIDTH, 1),
                        y=round(float(line["y"]) * HEIGHT, 1),
                        w=round(float(line["w"]) * WIDTH, 1),
                        h=round(float(line["h"]) * HEIGHT, 1),
                    )
                    for line in record["lines"]
                ]
    return found


def write_text(path: Path, frames: list[Frame]) -> None:
    with path.open("w") as out:
        for frame in frames:
            lines = [[l.text, l.x, l.y, l.w, l.h] for l in frame.lines]
            out.write(json.dumps({"t": frame.t, "lines": lines}) + "\n")


def read_text(path: Path) -> list[Frame]:
    frames: list[Frame] = []
    for record in map(json.loads, path.read_text().splitlines()):
        lines = [Line(str(l[0]), *map(float, l[1:])) for l in record["lines"]]
        frames.append(Frame(float(record["t"]), lines))
    return frames


def sample(source: Path, out: Path, fps: float, jobs: int) -> list[Frame]:
    """Decodes the recording at `fps` into `out`/frames and reads each frame's text."""
    stills = out / "frames"
    shutil.rmtree(stills, ignore_errors=True)
    stills.mkdir(parents=True)
    print(f"decoding at {fps:g} fps", flush=True)
    ffmpeg(
        *("-i", str(source), "-vf", f"fps={fps:g},{TABLE}"),
        *("-q:v", "4", f"{stills}/%06d.jpg"),
    )
    images = sorted(stills.glob("*.jpg"))
    print(f"reading {len(images)} frames", flush=True)
    text = recognize(images, jobs)
    # The fps filter's nth frame is the one nearest (n - 1) / fps.
    return [
        Frame(round(n / fps, 3), text.get(image, [])) for n, image in enumerate(images)
    ]


def shot_name(t: float) -> str:
    return f"{clock(t).replace(':', '-')}.jpg"


def save_shots(source: Path, out: Path, events: list[Event], fps: float) -> None:
    """A frame per event into `out`/shots: the sampled one while `sample`'s are there, else
    cut from the recording. A stack event's is half a second in, once its card has landed.
    """
    shots = out / "shots"
    shutil.rmtree(shots, ignore_errors=True)
    shots.mkdir(parents=True)
    wanted: dict[str, float] = {}
    for event in events:
        late = event.kind in ("spell", "ability") and event.end - event.t >= 0.5
        t = event.t + 0.5 if late else event.t
        event.shot = f"shots/{shot_name(t)}"
        wanted[event.shot] = t

    def save(item: tuple[str, float]) -> None:
        name, t = item
        still = out / "frames" / f"{round(t * fps) + 1:06d}.jpg"
        if still.exists():
            shutil.copy(still, out / name)
        else:
            args = ("-vf", TABLE, "-frames:v", "1", "-q:v", "4", str(out / name))
            ffmpeg("-ss", f"{t:.3f}", "-i", str(source), *args)

    with ThreadPoolExecutor(max_workers=8) as pool:
        list(pool.map(save, wanted.items()))
    shutil.rmtree(out / "frames", ignore_errors=True)


def index(args: argparse.Namespace) -> int:
    """Indexes the recording named, or with --new each one not yet indexed, in turn: each
    takes every core, so they never run together."""
    if args.new == bool(args.video):
        print("name one recording, or pass --new", file=sys.stderr)
        return 2
    if not args.new:
        return index_one(args.video, args)
    fresh = [name for name in known() if not (root() / name / "timeline.txt").is_file()]
    failed = 0
    for n, name in enumerate(fresh, 1):
        print(f"[{n}/{len(fresh)}] {name}", flush=True)
        failed += index_one(name, args) != 0
    if not fresh:
        print("every recording is indexed")
    return 1 if failed else 0


def index_one(name: str, args: argparse.Namespace) -> int:
    try:
        source, out = resolve(name)
        out.mkdir(parents=True, exist_ok=True)
        text = out / "text.jsonl"
        meta = out / "meta.json"
        fps = float(args.fps)
        old = json.loads(meta.read_text()) if meta.is_file() else {}
        if args.force or not text.is_file() or old.get("fps") != fps:
            frames = sample(source, out, fps, int(args.jobs))
            write_text(text, frames)
            meta.write_text(json.dumps({"source": str(source), "fps": fps}) + "\n")
        frames = read_text(text)
        events = derive(frames)
        save_shots(source, out, events, fps)
    except VideoError as error:
        print(error, file=sys.stderr)
        return 1
    with (out / "events.jsonl").open("w") as stream:
        for event in events:
            record = {key: value for key, value in asdict(event).items() if value}
            stream.write(
                json.dumps({"t": event.t, "at": clock(event.t), **record}) + "\n"
            )
    (out / "timeline.txt").write_text(timeline(events))
    kinds = Counter(event.kind for event in events)
    print(", ".join(f"{count} {kind}" for kind, count in sorted(kinds.items())))
    print(out / "timeline.txt")
    return 0


def find(args: argparse.Namespace) -> int:
    """Prints the timeline's matching lines, then the spans of frames whose text matches."""
    try:
        _, out = resolve(args.video)
    except VideoError as error:
        print(error, file=sys.stderr)
        return 1
    pattern: re.Pattern[str] = re.compile(args.pattern, re.IGNORECASE)
    for line in (out / "timeline.txt").read_text().splitlines():
        if pattern.search(line):
            print(line)
    frames = read_text(out / "text.jsonl")

    def match(frame: Frame) -> str:
        return next((l.text for l in frame.lines if pattern.search(l.text)), "")

    print("on screen:")
    for run in runs([match(frame) for frame in frames]):
        start, end = frames[run.indices[0]].t, frames[run.indices[-1]].t
        print(f"{clock(start):>9} to {clock(end)}  {run.text}")
    return 0


def listing(_: argparse.Namespace) -> int:
    """Prints each recording's id, whether it is indexed, and its name."""
    found = known()
    for name, source in found.items():
        indexed = (root() / name / "timeline.txt").is_file()
        print(f"{name:<14} {'indexed' if indexed else '-':<8} {source.name}")
    if not found:
        print(f"no recordings in {root()} or {MOVIES}", file=sys.stderr)
    return 0


def film(args: argparse.Namespace) -> int:
    """Tiles the frames from `at` on into one image, left to right then down, and prints its
    path and each row's times."""
    try:
        source, out = resolve(args.video)
        start = parse_clock(args.at)
        fps, span, cols = float(args.fps), float(args.span), int(args.cols)
        count = max(1, round(span * fps))
        cols = min(cols, count)
        rows = math.ceil(count / cols)
        filters = [f"fps={fps:g}", TABLE]
        if args.crop:
            x, y, w, h = (int(part) for part in args.crop.split(","))
            filters.append(f"crop={w}:{h}:{x}:{y}")
        filters += [f"scale={int(args.width)}:-2", f"tile={cols}x{rows}:padding=4"]
        name = f"{shot_name(start)[:-4]}-{span:g}s-{fps:g}fps"
        name += f"-{args.crop.replace(',', '_')}" if args.crop else ""
        sheet = out / "film" / f"{name}.jpg"
        sheet.parent.mkdir(parents=True, exist_ok=True)
        ffmpeg(
            *("-ss", f"{start:.3f}", "-t", f"{count / fps:.3f}", "-i", str(source)),
            *("-vf", ",".join(filters), "-frames:v", "1", "-q:v", "3", str(sheet)),
        )
    except (VideoError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    print(sheet)
    for row in range(rows):
        times = [
            start + n / fps for n in range(row * cols, min(count, (row + 1) * cols))
        ]
        print(f"row {row + 1}: " + " ".join(clock(t, 2) for t in times))
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    from . import video_cards, video_measure  # They import this module.

    video = sub.add_parser("video", help="index an Arena recording as a UI reference")
    commands = video.add_subparsers(dest="video_command", required=True)

    p = commands.add_parser("list", help="the recordings and which are indexed")
    p.set_defaults(func=listing)
    video_cards.register(commands)

    p = commands.add_parser("index", help="write a recording's text, events and shots")
    p.add_argument("video", nargs="?", help="the recording or its id")
    p.add_argument("--new", action="store_true", help="every recording not yet indexed")
    p.add_argument("--fps", default="2", help="samples a second (default 2)")
    p.add_argument("--jobs", default="6", help="recognizer processes (default 6)")
    p.add_argument("--force", action="store_true", help="read the frames again")
    p.set_defaults(func=index)

    p = commands.add_parser(
        "find", help="search a recording's timeline and on-screen text"
    )
    p.add_argument("video", help="the recording or its id")
    p.add_argument("pattern", help="a regular expression, case-insensitive")
    p.set_defaults(func=find)

    p = commands.add_parser(
        "film", help="tile the frames around a moment into one image"
    )
    p.add_argument("video", help="the recording or its id")
    p.add_argument("at", help="where to start: 24:32, 1:02:03.5 or seconds")
    p.add_argument("--span", default="3", help="seconds to cover (default 3)")
    p.add_argument("--fps", default="10", help="frames a second (default 10)")
    p.add_argument("--cols", default="6", help="frames a row (default 6)")
    p.add_argument("--width", default="480", help="each frame's width (default 480)")
    p.add_argument("--crop", help="x,y,w,h in the table's pixels")
    p.set_defaults(func=film)

    video_measure.register(commands)
