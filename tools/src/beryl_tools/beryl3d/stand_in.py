"""`just b3d stand-in`: the desktop app plays a whole game against `expert`, with this script
standing in for the person (docs/beryl-3d.md, "Manual QA", "A whole game on the desktop").

The script starts the debug app with no window, picks `expert` on the start screen and answers
every prompt through the app's live socket (`just b3d live`), choosing by a fixed rule: the
primary button when it is enabled, else the first enabled button of the prompt, else the first
eligible card or element the tree lists. After `RESUME_AFTER` prompts it quits the app, starts
it again and checks that the game resumed with the same board, then plays on to the game over
screen. It prints how many prompts it answered, the outcome and the sounds the frame logs
counted. Each run must end with no card drawn blank for want of its face and no glyph missing
from the text (the frame log's `faceless_cards` and `missing_glyphs`): the tree alone cannot
show that a host failed to deliver its card faces or glyphs.

The app runs under a profile of its own (`BERYL_PROFILE`), so that it starts on the start
screen with no game to resume and never touches the player's games. The profile's folder is
deleted when the run passes.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import threading
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

from .. import checks
from . import app, live, paths

# The agent the stand-in plays against, which the start screen lists only when `--offer`
# adds it.
OPPONENT = "expert"
# How long a prompt may stay open, unchanged, on the app's clock before the run fails.
PROMPT_LIMIT_MS = 60_000
# How long the game may sit with no prompt and nothing changing on screen, on the app's clock,
# before the run fails: the opponent answers in moments, so this is a stall.
STALL_LIMIT_MS = 120_000
# How long, on the app's clock, a card may rest in `nowhere` while a prompt is open before the
# run fails: longer than any step that moves a card away takes to play.
NOWHERE_GRACE_MS = 1_000
# The states a pointer gives an element, which a press changes before its answer is taken.
POINTER_STATES: frozenset[str] = frozenset({"hovered", "held"})
# How many prompts are answered before the app is quit and started again.
RESUME_AFTER = 20
# How long each run of the app may last before it ends itself: a bound on a hang.
RUN_LIMIT_S = 1800
# How long to wait for the app to start, show a screen or change after an answer, in seconds
# of real time.
START_LIMIT_S = 60.0
CHANGE_LIMIT_S = 5.0
POLL_S = 0.1
# One line of a tree: its indent, role, optional quoted name and id, then place, box and states.
LINE: re.Pattern[str] = re.compile(
    r'^(?P<indent> *)(?P<role>[a-z]+)(?: "(?P<name>(?:[^"\\]|\\.)*)")?(?: #(?P<id>\d+))?(?P<rest>.*)$'
)
# The words of a line after its box (`12,34 56x78`): its states.
BOX: re.Pattern[str] = re.compile(r"-?\d+,-?\d+ \d+x\d+")


class StandInError(Exception):
    """Why the run failed."""


@dataclass(frozen=True)
class Element:
    """One line of a tree."""

    depth: int
    role: str
    name: str | None
    id: str | None
    place: str
    states: frozenset[str]

    @property
    def locator(self) -> str:
        """The locator `live do` takes for this element."""
        out = self.role
        if self.name is not None:
            out += f' "{self.name}"'
        if self.id is not None:
            out += f" #{self.id}"
        return out


@dataclass
class Tree:
    """A tree as `live tree` prints it: the app's clock and the elements, in order."""

    clock_ms: int
    elements: list[Element]
    text: str

    def region(self, name: str) -> list[Element] | None:
        """The elements inside the first region named `name`, or None when there is none."""
        for at, element in enumerate(self.elements):
            if element.role == "region" and element.name == name:
                inside = []
                for other in self.elements[at + 1 :]:
                    if other.depth <= element.depth:
                        break
                    inside.append(other)
                return inside
        return None

    def named(self, role: str) -> list[str]:
        return [e.name for e in self.elements if e.role == role and e.name is not None]


def parse_tree(text: str) -> Tree:
    """Reads the answer to `live tree`: a first line `t=<ms>ms`, then one element a line."""
    first, _, rest = text.partition("\n")
    if not (first.startswith("t=") and first.endswith("ms")):
        raise StandInError(f"the tree has no clock line: {first!r}")
    elements = []
    for line in rest.splitlines():
        match = LINE.match(line)
        if not match or not line.strip():
            continue
        after = match.group("rest").strip()
        box = BOX.search(after)
        place = after[: box.start()].strip() if box else ""
        words = after[box.end() :].split() if box else after.split()
        elements.append(
            Element(
                depth=len(match.group("indent")) // 2,
                role=match.group("role"),
                name=match.group("name"),
                id=match.group("id"),
                place=place,
                states=frozenset(word.split("(")[0] for word in words),
            )
        )
    return Tree(int(first[2:-2]), elements, rest)


def choose(tree: Tree) -> Element | None:
    """What the fixed rule answers the open prompt with, or None when no prompt is open: the
    primary button when it is enabled, else the first enabled button, else the first eligible
    element."""
    prompt = tree.region("prompt")
    if prompt is None:
        return None
    buttons = [e for e in prompt if e.role == "button" and "disabled" not in e.states]
    primary = [e for e in buttons if "primary" in e.states]
    if primary or buttons:
        return (primary or buttons)[0]
    eligible = [e for e in tree.elements if "eligible" in e.states]
    return eligible[0] if eligible else None


def prompt_key(tree: Tree) -> tuple[str, ...]:
    """What identifies the open prompt: its elements and the eligible ones, with their states
    but not those the pointer gives. Pressing an answer changes it once the answer is taken.
    """
    prompt = tree.region("prompt") or []
    eligible = [e for e in tree.elements if "eligible" in e.states]
    return tuple(
        f"{e.locator} {' '.join(sorted(e.states - POINTER_STATES))}"
        for e in [*prompt, *eligible]
    )


def nowhere(place: str) -> bool:
    """Whether `place` is `nowhere`, where a card goes that has left every zone of the game."""
    return place.split(".")[0] == "nowhere"


def resting_nowhere(tree: Tree) -> list[Element]:
    """The cards the tree shows at rest in `nowhere`. A card that has left every zone may be
    drawn there while a step moves it away, but none should stay: the client drops the object
    once its script has played."""
    return [
        e
        for e in tree.elements
        if e.role == "card" and nowhere(e.place) and "moving" not in e.states
    ]


def board(tree: Tree) -> Counter[str]:
    """What must be the same after a resume: the life totals and pile counts (players and
    texts), and each card by its name, or its id when it is face down, and place, leaving out
    the faces in the Revealed Cards pile (place `revealed`), which a resumed client starts
    without. The tree shows no turn number, so the cards in each zone stand for it.
    """
    seen: Counter[str] = Counter()
    for element in tree.elements:
        words = (element.name or "").split()
        counted = (
            element.role == "text" and bool(words) and words[-1].lstrip("-").isdigit()
        )
        if element.role == "player" or counted:
            seen[f"{element.role} {element.name}"] += 1
        elif element.role == "card" and element.place and element.place != "revealed":
            seen[f"card {element.name or '#' + str(element.id)} {element.place}"] += 1
    return seen


@dataclass
class Running:
    """One run of the app, with its output read on a thread."""

    process: subprocess.Popen[str]
    lines: list[str] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def read(self) -> None:
        output = self.process.stdout
        if output is None:
            return
        for line in output:
            with self.lock:
                self.lines.append(line.rstrip("\n"))

    def line(self, prefix: str) -> str | None:
        with self.lock:
            return next((l for l in self.lines if l.startswith(prefix)), None)

    def wait_line(self, prefix: str, limit: float = START_LIMIT_S) -> str:
        deadline = time.monotonic() + limit
        while time.monotonic() < deadline:
            found = self.line(prefix)
            if found is not None:
                return found
            if self.process.poll() is not None:
                break
            time.sleep(POLL_S)
        raise StandInError(
            f"the app never printed {prefix!r}; its output:\n" + "\n".join(self.lines)
        )

    @property
    def socket(self) -> Path:
        line = self.wait_line(app.LIVE_SOCKET)
        return Path(line.removeprefix(f"{app.LIVE_SOCKET} "))

    def ask(self, line: str) -> str:
        """Sends `line` to the live socket, failing the run when the app refuses it."""
        try:
            accepted, body = live.request(self.socket, line)
        except OSError as error:
            raise StandInError(
                f"live {line}: the app did not answer: {error}"
            ) from error
        if not accepted:
            raise StandInError(f"live {line}: {body}")
        return body

    def tree(self) -> Tree:
        return parse_tree(self.ask("tree"))

    def act(self, action: str) -> None:
        self.ask(f"do {action}")

    def quit(self) -> list[str]:
        """Quits the app as closing its window does and gives all it printed."""
        self.ask("quit")
        try:
            self.process.wait(timeout=START_LIMIT_S)
        except subprocess.TimeoutExpired as error:
            self.process.kill()
            raise StandInError("the app did not end after quit") from error
        with self.lock:
            return list(self.lines)


def start(profile: str) -> Running:
    command = [str(app.BINARY), "--window", "none", "--for", f"{RUN_LIMIT_S}s"]
    command += ["--offer", OPPONENT]
    process = subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env={**os.environ, "BERYL_PROFILE": profile},
    )
    running = Running(process)
    threading.Thread(target=running.read, daemon=True).start()
    running.wait_line(app.FIRST_SHOWN)
    return running


def wait_tree(running: Running, ready: str, limit: float = START_LIMIT_S) -> Tree:
    """Polls the tree until `ready` says it is the screen wanted."""
    deadline = time.monotonic() + limit
    while True:
        tree = running.tree()
        if ready_for(tree, ready):
            return tree
        if time.monotonic() > deadline:
            raise StandInError(f"no {ready} screen within {limit:.0f} s:\n{tree.text}")
        time.sleep(POLL_S)


def ready_for(tree: Tree, ready: str) -> bool:
    if ready == "start":
        return tree.region("start") is not None
    if ready == "game":
        return tree.region("game") is not None
    if ready == "prompt":
        return choose(tree) is not None or tree.region("game over") is not None
    raise ValueError(ready)


def pick_opponent(running: Running, title: str = "Expert") -> None:
    """Chooses the opponent whose toggle is named `title` (`expert`'s is "Expert") on the start
    screen, scrolling the list to it, and presses Start."""
    tree = wait_tree(running, "start")
    if title not in tree.named("toggle"):
        running.act('wheel list "Opponent" 0,-4000')
    running.act(f'press toggle "{title}"')
    running.act('press button "Start"')


@dataclass
class Played:
    """What the stand-in did: the prompts it answered, the outcome, and a file listing each
    prompt and its answer."""

    transcript: Path
    prompts: int = 0
    outcome: str | None = None

    def log(self, tree: Tree, answer: Element) -> None:
        prompt = [e.locator for e in tree.region("prompt") or [] if e.role == "text"]
        with self.transcript.open("a", encoding="utf-8") as file:
            file.write(
                f"{self.prompts + 1} t={tree.clock_ms}ms {' | '.join(prompt)} -> {answer.locator}\n"
            )


def play(running: Running, played: Played, stop_at: int | None) -> Tree:
    """Answers prompts until the game is over or `stop_at` prompts have been answered. Gives
    the tree it stopped at, with a prompt open or the game over."""
    since: tuple[tuple[str, ...], int] | None = None
    deadline = time.monotonic() + RUN_LIMIT_S
    while time.monotonic() < deadline:
        tree = running.tree()
        over = tree.region("game over")
        if over is not None:
            texts = [e.name for e in over if e.role == "text" and e.name]
            played.outcome = " / ".join(texts) or "over"
            return tree
        answer = choose(tree)
        key = prompt_key(tree) if answer else (tree.text,)
        if since is None or since[0] != key:
            since = (key, tree.clock_ms)
        elif tree.clock_ms - since[1] > (PROMPT_LIMIT_MS if answer else STALL_LIMIT_MS):
            what = (
                "a prompt stayed open"
                if answer
                else "nothing changed and no prompt opened"
            )
            limit = (PROMPT_LIMIT_MS if answer else STALL_LIMIT_MS) // 1000
            raise StandInError(f"{what} for {limit} s of the app's clock:\n{tree.text}")
        if answer is None:
            time.sleep(POLL_S)
            continue
        if resting_nowhere(tree):
            check_nowhere(running)
        if stop_at is not None and played.prompts >= stop_at:
            return tree
        played.log(tree, answer)
        running.act(f"press {answer.locator}")
        played.prompts += 1
        wait_change(running, key)
    raise StandInError(f"the game did not end within {RUN_LIMIT_S} s")


def check_nowhere(running: Running) -> None:
    """Fails the run when a card still rests in `nowhere` after the table has had
    `NOWHERE_GRACE_MS` of the app's clock to finish what it was playing."""
    first = running.tree()
    tree = first
    while tree.clock_ms - first.clock_ms < NOWHERE_GRACE_MS:
        time.sleep(POLL_S)
        tree = running.tree()
    left = resting_nowhere(tree)
    if left:
        cards = ", ".join(e.locator for e in left)
        raise StandInError(
            f"{cards} rested in nowhere with a prompt open, where no card should stay:\n"
            + tree.text
        )


def wait_change(running: Running, before: tuple[str, ...]) -> None:
    """Waits until the prompt differs from `before`, its answer having been taken, or a few
    seconds have passed (the next prompt may look the same)."""
    deadline = time.monotonic() + CHANGE_LIMIT_S
    while time.monotonic() < deadline:
        if prompt_key(running.tree()) != before:
            return
        time.sleep(POLL_S)


def settled(running: Running) -> Tree:
    """The tree once a prompt or the game over shows and two looks in a row agree."""
    last = wait_tree(running, "prompt")
    while True:
        time.sleep(0.5)
        tree = wait_tree(running, "prompt")
        if board(tree) == board(last):
            return tree
        last = tree


def frame_log(output: list[str]) -> dict[str, object]:
    """The summary of the frame log a run printed the path of."""
    line = next((l for l in output if l.startswith("frame log: ")), None)
    if line is None:
        raise StandInError("the app printed no frame log:\n" + "\n".join(output))
    lines = Path(line.removeprefix("frame log: ")).read_text(encoding="utf-8")
    for entry in reversed(lines.splitlines()):
        summary = json.loads(entry).get("summary")
        if isinstance(summary, dict):
            return summary
    raise StandInError(f"{line}: no summary")


def check_assets(run: str, summary: dict[str, object]) -> None:
    """Fails when the last frame of a run, well after its opening hand, still drew a card blank
    for want of its face or lacked a glyph."""
    faceless = summary.get("faceless_cards", 0)
    missing = summary.get("missing_glyphs", 0)
    if faceless or missing:
        raise StandInError(
            f"the {run} run ended with {faceless} cards drawn blank with no face and "
            f"{missing} glyphs missing: the host is not delivering its assets"
        )


def data_folder(running: Running) -> Path | None:
    found = running.line(app.DATA_IN)
    return Path(found.removeprefix(app.DATA_IN)) if found else None


def stand_in_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--no-build", action="store_true")


def run_stand_in(args: argparse.Namespace) -> int:
    if not args.no_build and app.build() != 0:
        return checks.CANNOT_RUN
    profile = f"b3d-stand-in-{os.getpid()}"
    folder: Path | None = None
    first: Running | None = None
    second: Running | None = None
    try:
        out = paths.cache() / "out"
        out.mkdir(parents=True, exist_ok=True)
        played = Played(out / f"stand-in-{os.getpid()}.txt")
        print(
            f"stand-in: each prompt and its answer go to {played.transcript}",
            flush=True,
        )
        first = start(profile)
        folder = data_folder(first)
        pick_opponent(first)
        play(first, played, RESUME_AFTER)
        before = board(settled(first))
        first_output = first.quit()
        print(
            f"stand-in: quit after {played.prompts} prompts; starting again", flush=True
        )
        second = start(profile)
        after = board(settled(second))
        if after != before:
            missing = sorted((before - after).elements())
            extra = sorted((after - before).elements())
            raise StandInError(
                f"the resumed game differs: before only {missing}, after only {extra}"
            )
        print(
            f"stand-in: resumed with the same board ({sum(after.values())} lines)",
            flush=True,
        )
        play(second, played, None)
        second_output = second.quit()
        sounds: Counter[str] = Counter()
        for run, output in (("first", first_output), ("second", second_output)):
            summary = frame_log(output)
            check_assets(run, summary)
            counted = summary.get("sounds")
            if isinstance(counted, dict):
                for name, times in counted.items():
                    if isinstance(name, str) and isinstance(times, int):
                        sounds[name] += times
        print(f"stand-in: {played.prompts} prompts answered; outcome: {played.outcome}")
        print(
            "stand-in: sounds played: "
            + (", ".join(f"{name} {n}" for name, n in sorted(sounds.items())) or "none")
        )
        if not sounds:
            raise StandInError("the frame logs counted no sound")
    except StandInError as error:
        print(f"stand-in: FAILED: {error}")
        if folder is not None:
            print(f"stand-in: the profile's data is kept in {folder}")
        return 1
    finally:
        for running in (first, second):
            if running is not None and running.process.poll() is None:
                running.process.kill()
                running.process.wait()
    if folder is not None:
        shutil.rmtree(folder, ignore_errors=True)
    print("stand-in: passed")
    return 0
