"""`just b3d motion-film <verb> [case]`: Beryl 3D's frames of a verb beside Arena's, for the
motion-review and arena-pixel-match skills, as `just motion --film` films the web interface's
(tools/src/beryl_tools/motion_film.py).

It finds a recording that plays the verb (the verb's entry in `data/beryl3d/parity.json`, or
`--recording`), reads Beryl 3D's timeline of it from each seat (`just b3d timeline`, with each
view's start from the steps `just b3d info` times), and films
the first step of each case of the verb, or of the one case asked for, from its start to
`SETTLE_MS` past its end, or the recording's end if that comes first. Each step is filmed from both seats in both layouts with `just b3d film`
at Arena's rate and width (`--rate 15 --frame-width 760`), in films of at most `MOST_FRAMES`
frames each. Arena's instances of the same verb and case (data/arena-motion.json) are cut from
their recordings by the main package's `retake` and `arena_frames`, into
`$BERYL_CACHE/b3d/motion-film/<verb>-<run>/arena/`. When no instance's recording is on this
machine (`$BERYL_ARENA`), it films Beryl 3D's side alone and says so on one line.

It prints, for each case, the folder of each Arena instance's frames beside the paths of
Beryl 3D's films, and then each film's text: its layout, seat and case, the step and the steps
playing meanwhile, the picture's path and what moves in each frame. Never commit the images.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .. import motion, video
from .. import motion_film as web_film
from . import paths
from .command import Command

PARITY: Path = paths.DATA / "parity.json"
SEATS: tuple[str, ...] = ("you", "opponent")
LAYOUTS: tuple[str, ...] = ("desktop", "phone")
# The most frames one film holds (`MOST_FRAMES` in beryl_pictures' film.rs). A step longer than
# that at Arena's rate is filmed in several films, one after another.
MOST_FRAMES = 24
# Milliseconds filmed past a step's end, where its subject comes to rest, as Arena's instances
# are cut.
SETTLE_MS: float = web_film.SETTLE * 1000
# How many steps playing meanwhile a film's text names before it counts the rest.
MOST_MEANWHILE = 4
# The lines of `just b3d info` that give a step's start, in ms since the recording's start,
# and the recording's end.
STEP_LINE: re.Pattern[str] = re.compile(
    r"^step:(\d+)\.(\d+)\.(\d+)\s.* from ([\d.]+)ms to [\d.]+ms$"
)
END_LINE: re.Pattern[str] = re.compile(r"ends at ([\d.]+)ms")

Step = dict[str, Any]


@dataclass
class Film:
    """One film of Beryl 3D's: a step from one seat in one layout, as one or more pictures."""

    layout: str
    seat: str
    step: Step
    meanwhile: list[Step]
    # When the step begins and the film ends, in ms since the recording's start.
    start: float
    end: float
    pictures: list[str] = field(default_factory=list)
    # Each picture's frame lines, `#<n> <ms>  <what moves>`.
    lines: list[str] = field(default_factory=list)


def label(step: Step) -> str:
    case = f" ({step['case']})" if step.get("case") else ""
    return f"{step['verb']}{case} {step['start']:+.0f} to {step['end']:+.0f}"


def milliseconds(value: float) -> str:
    """A number of milliseconds as a duration `b3d` reads: whole when it is, else to 3 places."""
    return f"{value:.0f}ms" if value == int(value) else f"{value:.3f}ms"


def spans(start: float, end: float) -> list[tuple[float, float]]:
    """The spans, in ms, of the films of a step from `start` to `end`: each at most
    `MOST_FRAMES` frames at Arena's rate, the last ending at `end`."""
    longest = MOST_FRAMES * 1000 / web_film.ARENA_FPS
    out: list[tuple[float, float]] = []
    at = start
    while True:
        out.append((at, min(at + longest, end)))
        at += longest
        if at >= end:
            return out


def film_arguments(
    recording: str, seat: str, layout: str, span: tuple[float, float]
) -> list[str]:
    """The `b3d-render` command line that films a span of a recording, in ms from its start."""
    return [
        "film",
        recording,
        *("--seat", seat, "--layout", layout),
        *("--from", milliseconds(span[0]), "--to", milliseconds(span[1])),
        *("--rate", str(web_film.ARENA_FPS)),
        *("--frame-width", str(web_film.ARENA_WIDTH)),
    ]


def first_steps(steps: list[Step], verb: str, case: str | None) -> list[Step]:
    """The first step of each case of `verb` in a timeline, or of `case` alone."""
    seen: set[str | None] = set()
    out: list[Step] = []
    for step in steps:
        if step["verb"] != verb or (case is not None and step.get("case") != case):
            continue
        if step.get("case") not in seen:
            seen.add(step.get("case"))
            out.append(step)
    return out


def meanwhile(steps: list[Step], subject: Step) -> list[Step]:
    """The other steps of the subject's view that play while it does."""
    return [
        s
        for s in steps
        if s["view"] == subject["view"]
        and s is not subject
        and (s["verb"], s.get("case")) != (subject["verb"], subject.get("case"))
        and s["start"] < subject["end"]
        and s["end"] > subject["start"]
    ]


def run_built(name: str, arguments: list[str]) -> str:
    """Runs the built program `name` and returns what it printed. When it says the recordings
    pack is missing, builds the pack and runs it again; any other failure ends the command.
    """
    from . import cli

    line = [str(cli.linked(name)), *arguments]
    done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode == cli.MISSING_PACK:
        if cli.record() != 0:
            raise SystemExit(1)
        done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        sys.stderr.write(done.stderr)
        raise SystemExit(done.returncode)
    return done.stdout


def build() -> int:
    from . import cli

    for package, binary in (("beryl_inspect", "b3d"), ("beryl_pictures", "b3d-render")):
        built = cli.quick_build(["build", "-q", "-p", package, "--bin", binary])
        if built != 0:
            return built
    return 0


def offsets(steps: list[Step], printed: list[str]) -> dict[int, float]:
    """How far into the recording each view's timeline starts, in ms. A timeline times its
    steps from its view's start, and `just b3d info` lists some of the same steps (by view,
    beat and step) from the recording's start."""
    started = {
        (int(m[1]), int(m[2]), int(m[3])): float(m[4])
        for m in (STEP_LINE.match(line.strip()) for line in printed)
        if m
    }
    found: dict[int, float] = {}
    for step in steps:
        at = started.get((step["view"], step["beat"], step["step"]))
        if at is not None:
            found.setdefault(step["view"], at - step["start"])
    return found


def moments(
    recording: str, seat: str, steps: list[Step]
) -> tuple[dict[int, float], float]:
    """How far into the recording each view's timeline starts, from one seat, and when the
    recording ends, in ms since its start."""
    printed = run_built("b3d", ["info", recording, "--seat", seat]).splitlines()
    end = next(float(m[1]) for m in map(END_LINE.search, printed) if m)
    return offsets(steps, printed), end


def timeline(recording: str, seat: str) -> list[Step]:
    printed = run_built("b3d", ["timeline", recording, "--seat", seat])
    return [json.loads(line) for line in printed.splitlines() if line.startswith("{")]


def shoot(recording: str, film: Film) -> None:
    """Films each span of the film's step and keeps each picture's path and frame lines."""
    for span in spans(film.start, film.end):
        arguments = film_arguments(recording, film.seat, film.layout, span)
        for line in run_built("b3d-render", arguments).splitlines():
            if line.startswith("wrote "):
                film.pictures.append(line.removeprefix("wrote ").split(" (")[0])
            elif line.startswith("  #"):
                film.lines.append(line.strip())


def film_lines(recording: str, film: Film) -> list[str]:
    """One film's text, as `take_lines` writes a web take's in `just motion --film`."""
    step = film.step
    shown = [label(s) for s in film.meanwhile[:MOST_MEANWHILE]]
    rest = len(film.meanwhile) - len(shown)
    return [
        f"{film.layout} / {film.seat} / {step.get('case') or step['verb']}"
        f"   (recording {recording}, view {step['view']} at {film.start:.0f} ms,"
        f" {step['end'] - step['start']:.0f} ms)",
        f"  step: {label(step)}",
        "  playing meanwhile: "
        + (", ".join(shown) + (f" and {rest} more" if rest else "") or "nothing"),
        *(f"  film: {path}" for path in film.pictures),
        *(f"  {line}" for line in film.lines),
    ]


def arena_side(
    verb: str, case: str | None, most: int, folder: Path
) -> tuple[list[dict[str, Any]], list[str]]:
    """Arena's instances of the verb cut into `folder`, and why each one left out was."""
    cut: list[dict[str, Any]] = []
    missing: list[str] = []
    for n, instance in enumerate(web_film.chosen(motion.motion(), verb, case, most)):
        take = web_film.retake(instance)
        name = f"{instance.get('case')} · {take.recording} {video.clock(take.start, 2)}"
        try:
            frames = web_film.arena_frames(take, folder / f"{n + 1:02d}")
        except video.VideoError as error:
            missing.append(f"{name}: {str(error).splitlines()[0]}")
            continue
        cut.append({"case": instance.get("case"), "label": name, **frames})
    return cut, missing


def run_motion_film(args: argparse.Namespace) -> int:
    verb = str(args.verb)
    case = " ".join(args.case) or None
    recording = args.recording or json.loads(PARITY.read_text())["verbs"].get(verb)
    if recording is None:
        print(
            f"motion-film: parity.json names no recording for {verb!r};"
            " give one with --recording",
            file=sys.stderr,
        )
        return 2
    if build() != 0:
        return 1
    films: list[Film] = []
    for seat in SEATS:
        steps = timeline(recording, seat)
        views, last = moments(recording, seat, steps)
        for step in first_steps(steps, verb, case):
            if step["view"] not in views:
                print(
                    f"motion-film: left out {label(step)} from the seat {seat}:"
                    f" `just b3d info` times no step of view {step['view']}"
                )
                continue
            start = views[step["view"]] + step["start"]
            end = min(views[step["view"]] + step["end"] + SETTLE_MS, last)
            for layout in LAYOUTS:
                films.append(
                    Film(layout, seat, step, meanwhile(steps, step), start, end)
                )
    if not films:
        which = f"{verb} ({case})" if case else verb
        print(f"motion-film: {recording} plays no {which}", file=sys.stderr)
        return 2
    for film in films:
        shoot(recording, film)
    # Named for the run, so that two runs at once do not cut Arena's frames into one folder.
    out = paths.cache() / "motion-film" / f"{verb}-{time.time_ns()}"
    arena, missing = arena_side(verb, case, int(args.instances), out / "arena")

    cases = list(dict.fromkeys(film.step.get("case") for film in films))
    for name in cases:
        print(f"{verb} ({name}):" if name else f"{verb}:")
        for instance in (a for a in arena if a["case"] == name):
            count = len(instance["frames"])
            print(
                f"  Arena {instance['label']}: {out / 'arena'}"
                f"/{Path(instance['frames'][0]['path']).parent.name}"
                f" ({count} frames, {instance['width']} px wide)"
            )
        for film in (f for f in films if f.step.get("case") == name):
            for path in film.pictures:
                print(f"  Beryl 3D {film.layout}/{film.seat}: {path}")
    for why in missing:
        print(f"left out: {why}")
    if not arena:
        print(
            f"Arena: none of {verb}'s instances is on this machine ({video.root()}),"
            " so only Beryl 3D's side is filmed"
        )
    for film in films:
        print()
        print("\n".join(film_lines(recording, film)))
    return 0


def motion_film_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("verb", help="a verb of data/ui-conventions.json, as fly")
    parser.add_argument(
        "case", nargs="*", help="one case of the verb, as own spell cast"
    )
    parser.add_argument(
        "--recording",
        help="the recording to film, instead of the verb's entry in data/beryl3d/parity.json",
    )
    parser.add_argument(
        "--instances",
        default=web_film.INSTANCES,
        help=f"how many of Arena's instances of each case (default {web_film.INSTANCES})",
    )


COMMANDS: tuple[Command, ...] = (
    Command(
        "motion-film",
        "films a verb from both seats in both layouts at Arena's rate, beside Arena's frames",
        (
            ("just b3d motion-film fly", "every case of fly, beside Arena's"),
            (
                "just b3d motion-film fly own spell cast --recording verbs-zones",
                "one case, from a recording you name",
            ),
        ),
        run_motion_film,
        motion_film_arguments,
    ),
)
