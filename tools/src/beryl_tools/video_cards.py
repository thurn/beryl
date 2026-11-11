"""`by video cards` and `by video card`: where cards appear across the indexed recordings.

Both read every index in ~/arena (video.py) and match the text on screen against card names.

- `cards` counts, for each card of Beryl's own (data/cards.json), its casts, its abilities and
  where else it is seen. `--top N` ranks every Magic card instead, by Scryfall's list of names,
  fetched once into the cache.
- `card NAME` writes everything about one card to ~/arena/cards/: each time it is cast or one
  of its abilities goes on the stack, with whose turn it is, what follows and the shot to look
  at, then its other sightings. `--sheet` tiles those shots, numbered as the text numbers them.

A sighting's zone is where on screen the name was read: the stack's card, your hand along the
bottom, a draft pick, or `shown` for anything else large enough to read (a hover preview, a
browser, a revealed card). Names on battlefield cards are too small to read, so a permanent is
seen there only when hovered."""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

from . import scryfall
from .cards import OUTPUT as CARDS_JSON
from . import paths, video
from .video import Event, Frame, Line

CATALOG = "https://api.scryfall.com/catalog/card-names"
# Samples a name may go unread for within one sighting.
GAP = 6
# A misread name is its card's when this alike. Short names must be read exactly.
ALIKE = 0.88
FUZZY_LENGTH = 8
# Your hand's names are read below this line.
HAND_Y = 900.0
# Card names that Arena also writes on its own buttons and headings.
INTERFACE: frozenset[str] = frozenset(
    name.lower()
    for name in (
        "Cancel",
        "Library",
        "Graveyard",
        "Exile",
        "Victory",
        "Defeat",
        "Draft",
        "Sideboard",
        "Opponent",
        "Reward",
        "Done",
        "Undo",
        "Arena",
        "X",
    )
)
# How long after a stack event what happens still belongs to it.
AFTER = 10.0
FOLLOWING = 6
# Sightings this close are written as one stretch.
NEAR = 30.0
SHEET = 12
SHEET_COLS = 4
SHEET_WIDTH = 640


class Names:
    """Card names to recognize. `fuzzy` also takes near misreadings, which costs a comparison
    with every name: for a few cards, not the whole catalog."""

    def __init__(self, names: Iterable[str], fuzzy: bool) -> None:
        self.names: dict[str, str] = {name.lower(): name for name in names}
        self.fuzzy: bool = fuzzy
        self.cache: dict[str, str] = {}

    def match(self, text: str) -> str:
        """The card `text` names, or ""."""
        key = video.clean_name(text).lower()
        if key not in self.cache:
            self.cache[key] = self.names.get(key) or (
                self.near(key) if self.fuzzy else ""
            )
        return self.cache[key]

    def near(self, key: str) -> str:
        if len(key) < FUZZY_LENGTH:
            return ""
        for known, name in self.names.items():
            if abs(len(known) - len(key)) <= 2:
                if difflib.SequenceMatcher(None, key, known).ratio() >= ALIKE:
                    return name
        return ""


@dataclass
class Recording:
    name: str
    frames: list[Frame]
    events: list[Event]


@dataclass
class Sighting:
    """A stretch of one recording during which a card's name was read in one zone."""

    video: str
    zone: str
    t: float
    end: float


@dataclass
class Card:
    """What the recordings show of one card."""

    name: str
    # Its stack events, with the recording each is in.
    stack: list[tuple[str, Event]] = field(default_factory=list)
    seen: list[Sighting] = field(default_factory=list)

    def count(self, kind: str) -> int:
        return sum(1 for _, event in self.stack if event.kind == kind)

    def zone(self, zone: str) -> int:
        return sum(1 for sighting in self.seen if sighting.zone == zone)

    def videos(self) -> set[str]:
        return {v for v, _ in self.stack} | {s.video for s in self.seen}


def load(folder: Path) -> list[Recording]:
    """Every indexed recording in a folder."""
    found: list[Recording] = []
    for index in sorted(folder.iterdir()) if folder.is_dir() else []:
        if not (index / "timeline.txt").is_file():
            continue
        events: list[Event] = []
        for record in map(
            json.loads, (index / "events.jsonl").read_text().splitlines()
        ):
            events.append(
                Event(
                    t=float(record["t"]),
                    end=float(record.get("end", record["t"])),
                    kind=str(record["kind"]),
                    name=str(record["name"]),
                    detail=str(record.get("detail", "")),
                    buttons=[str(b) for b in record.get("buttons", [])],
                    shot=str(record.get("shot", "")),
                )
            )
        found.append(
            Recording(index.name, video.read_text(index / "text.jsonl"), events)
        )
    return found


def zone(line: Line, drafting: bool) -> str:
    if drafting:
        return "draft"
    if video.STACK_NAME.has(line):
        return "stack"
    return "hand" if line.y >= HAND_Y else "shown"


def find(recordings: list[Recording], names: Names) -> dict[str, Card]:
    """Each named card's stack events and sightings. A card's own stack events are not also
    sightings."""
    found: dict[str, Card] = {}
    for recording in recordings:
        for event in recording.events:
            name = names.match(event.name) if event.kind in ("spell", "ability") else ""
            if name:
                found.setdefault(name, Card(name)).stack.append((recording.name, event))
        open_at: dict[tuple[str, str], tuple[int, Sighting]] = {}
        for index, frame in enumerate(recording.frames):
            drafting = video.is_draft(frame)
            for line in frame.lines:
                if video.BUTTONS.has(line) or line.text.lower() in INTERFACE:
                    continue
                name = names.match(line.text)
                where = zone(line, drafting)
                if not name or where == "stack":
                    continue
                last = open_at.get((name, where))
                if last is not None and index - last[0] <= GAP:
                    last[1].end = frame.t
                    open_at[(name, where)] = (index, last[1])
                else:
                    sighting = Sighting(recording.name, where, frame.t, frame.t)
                    found.setdefault(name, Card(name)).seen.append(sighting)
                    open_at[(name, where)] = (index, sighting)
    return found


def pool() -> list[str]:
    """The names of Beryl's cards, a name per face."""
    data = json.loads(CARDS_JSON.read_text())
    return sorted(
        {str(face["name"]) for card in data["cards"] for face in card["faces"]}
    )


def catalog(get: scryfall.Get = scryfall.http_get) -> list[str]:
    """Every Magic card's name, a name per face, from the cache or once from Scryfall."""
    cached = paths.cache_root() / "video" / "card-names.json"
    if not cached.is_file():
        cached.parent.mkdir(parents=True, exist_ok=True)
        cached.write_text(json.dumps(get(CATALOG)["data"]))
    names = cast(list[str], json.loads(cached.read_text()))
    return sorted({face for name in names for face in name.split(" // ")})


def table(cards: list[Card]) -> str:
    """A row per card: casts, abilities, sightings by zone and the recordings it is in."""
    width = max([len(card.name) for card in cards] + [4])
    lines = [f"{'card':<{width}}  cast  abil  hand  shown  draft  videos"]
    for card in cards:
        lines.append(
            f"{card.name:<{width}}  {card.count('spell'):>4}  {card.count('ability'):>4}"
            f"  {card.zone('hand'):>4}  {card.zone('shown'):>5}  {card.zone('draft'):>5}"
            f"  {len(card.videos()):>6}"
        )
    return "\n".join(lines) + "\n"


def played(card: Card) -> int:
    """How often a card is seen in play: on the stack or in hand. `shown` is left out, as a
    keyword in another card's text reads as the card of that name."""
    return len(card.stack) + card.zone("hand")


def whose_turn(events: list[Event], t: float) -> str:
    banner = [e.name for e in events if e.kind == "turn" and e.t <= t + 1]
    if not banner:
        return ""
    return "opp turn" if banner[-1].upper().startswith("OPP") else "your turn"


def following(events: list[Event], event: Event) -> list[str]:
    """What happened from a stack event until `AFTER` seconds past its end, in brief."""
    brief: list[str] = []
    for other in events:
        if other == event or other.kind in ("turn", "match", "draft"):
            continue
        if event.t <= other.t <= event.end + AFTER:
            text = (
                other.name
                if other.kind in ("life", "prompt")
                else f"{other.kind} {other.name}"
            )
            if other.kind == "prompt" and other.buttons:
                text += f" [{', '.join(other.buttons)}]"
            brief.append(f"+{other.t - event.t:.0f}s {text}")
    return brief[:FOLLOWING]


def spans(sightings: list[Sighting]) -> str:
    """Sightings as times, those within `NEAR` seconds of each other as one stretch."""
    merged: list[list[float]] = []
    for sighting in sorted(sightings, key=lambda s: s.t):
        if merged and sighting.t - merged[-1][1] <= NEAR:
            merged[-1][1] = max(merged[-1][1], sighting.end)
        else:
            merged.append([sighting.t, sighting.end])
    return ", ".join(
        video.clock(t) + (f"-{video.clock(end)}" if end - t >= 1 else "")
        for t, end in merged
    )


def dump(card: Card, recordings: list[Recording]) -> str:
    """Everything about one card as text: its stack events numbered, each with its context
    and shot, then its other sightings. Ability texts are lettered and written once."""
    events = {recording.name: recording.events for recording in recordings}
    texts = Counter(e.detail for _, e in card.stack if e.kind == "ability" and e.detail)
    letters = {
        text: chr(ord("a") + n) for n, (text, _) in enumerate(texts.most_common(26))
    }
    types = Counter(e.detail for _, e in card.stack if e.kind == "spell" and e.detail)
    out = [
        f"# {card.name}",
        f"{card.count('spell')} casts, {card.count('ability')} abilities, "
        f"{len(card.videos())} recordings. Times are into each recording; shots are in "
        f"{video.root()}/<recording>/.",
    ]
    if types:
        out.append(f"type line: {types.most_common(1)[0][0]}")
    for text, letter in letters.items():
        out.append(f"ability {letter} ({texts[text]}x): {text}")
    number = 0
    for name in sorted(card.videos()):
        out.append(f"\n## {name}")
        for _, event in ((v, e) for v, e in card.stack if v == name):
            number += 1
            what = (
                "cast"
                if event.kind == "spell"
                else f"ability {letters.get(event.detail, '?')}"
            )
            parts = [
                f"#{number} {video.clock(event.t)} {what}",
                f"{event.end - event.t:.0f}s",
            ]
            parts.append(whose_turn(events[name], event.t))
            if event.buttons:
                parts.append(f"[{', '.join(event.buttons)}]")
            parts.append(event.shot)
            out.append("  ".join(part for part in parts if part))
            then = following(events[name], event)
            if then:
                out.append("    " + "; ".join(then))
        for where in ("hand", "shown", "draft"):
            seen = [s for s in card.seen if s.video == name and s.zone == where]
            if seen:
                out.append(f"{where}: {spans(seen)}")
    return "\n".join(out) + "\n"


def sheets(card: Card, folder: Path, slug: str) -> list[Path]:
    """The card's stack events' shots tiled `SHEET` to an image, in the dump's order."""
    shots = [
        video.root() / name / event.shot for name, event in ordered(card) if event.shot
    ]
    made: list[Path] = []
    height = SHEET_WIDTH * 9 // 16
    for n in range(0, len(shots), SHEET):
        group = shots[n : n + SHEET]
        sheet = folder / f"{slug}-{n // SHEET + 1}.jpg"
        inputs = [arg for shot in group for arg in ("-i", str(shot))]
        scaled = "".join(
            f"[{i}:v]scale={SHEET_WIDTH}:{height}[s{i}];" for i in range(len(group))
        )
        layout = "|".join(
            f"{i % SHEET_COLS * SHEET_WIDTH}_{i // SHEET_COLS * height}"
            for i in range(len(group))
        )
        tiles = "".join(f"[s{i}]" for i in range(len(group)))
        stack = f"{tiles}xstack=inputs={len(group)}:layout={layout}:fill=black"
        graph = scaled + (stack if len(group) > 1 else "[s0]null")
        video.ffmpeg(
            *inputs, "-filter_complex", graph, "-frames:v", "1", "-q:v", "3", str(sheet)
        )
        made.append(sheet)
    return made


def ordered(card: Card) -> list[tuple[str, Event]]:
    """The card's stack events in the dump's order: by recording, then time."""
    return sorted(card.stack, key=lambda item: (item[0], item[1].t))


def slug(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


def cards(args: argparse.Namespace) -> int:
    recordings = load(video.root())
    if not recordings:
        print(f"no indexed recordings in {video.root()}", file=sys.stderr)
        return 1
    if args.top:
        found = find(recordings, Names(catalog(), fuzzy=False))
        ranked = sorted(found.values(), key=lambda card: (-played(card), card.name))
        print(table(ranked[: int(args.top)]), end="")
    else:
        found = find(recordings, Names(pool(), fuzzy=True))
        print(table([found.get(name, Card(name)) for name in pool()]), end="")
    print(f"{len(recordings)} recordings")
    return 0


def card(args: argparse.Namespace) -> int:
    recordings = load(video.root())
    names = Names(str(args.name).split(" // "), fuzzy=True)
    found = find(recordings, names)
    if not found:
        print(f"{args.name}: not seen in {len(recordings)} recordings", file=sys.stderr)
        return 1
    whole = Card(str(args.name))
    for part in found.values():
        whole.stack += part.stack
        whole.seen += part.seen
    whole.stack = ordered(whole)
    folder = video.root() / "cards"
    folder.mkdir(parents=True, exist_ok=True)
    text = dump(whole, recordings)
    try:
        made = sheets(whole, folder, slug(whole.name)) if args.sheet else []
    except video.VideoError as error:
        print(error, file=sys.stderr)
        return 1
    for n, sheet in enumerate(made):
        text += (
            f"sheet {n + 1}: #{n * SHEET + 1} on, left to right then down: {sheet}\n"
        )
    (folder / f"{slug(whole.name)}.txt").write_text(text)
    print(text, end="")
    print(folder / f"{slug(whole.name)}.txt")
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = commands.add_parser(
        "cards", help="how often Beryl's cards appear in the recordings"
    )
    p.add_argument("--top", help="rank every Magic card instead, and print this many")
    p.set_defaults(func=cards)

    p = commands.add_parser("card", help="every appearance of one card, as text")
    p.add_argument("name", help="the card's name; both faces as `A // B`")
    p.add_argument(
        "--sheet", action="store_true", help="tile its stack events' shots too"
    )
    p.set_defaults(func=card)
