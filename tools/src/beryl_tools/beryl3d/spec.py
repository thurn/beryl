"""`just b3d spec <card> [moment]`: a card's spec moments as Beryl 3D shows them, in text.

The web interface's `just specs <card> --describe` lists each moment of a card's spec. Beryl 3D
already records every one as `<card>/<moment>` (`just b3d list <card>`); this command prints
what the `specs` skill's web side gives, from each seat: the prompts of `just b3d info`, the
`just b3d tree` at each prompt (cut to `TREE_LINES` lines) and the result of `just b3d check`.
`--film` also draws each moment (`just b3d film`) and prints the pictures' paths. It runs the
`just b3d` commands rather than the programs, so the cargo build and the recordings pack are
handled as for each of them alone.
"""

from __future__ import annotations

import argparse
import json
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass

from .. import specs
from ..paths import BY
from .command import Command

# The most lines of a tree printed for one prompt, so the output stays bounded.
TREE_LINES = 40
# How many frames `--film` draws of each moment.
FILM_FRAMES = 12
SEATS = ("you", "opponent")
PROMPT_LINE: re.Pattern[str] = re.compile(r"^(prompt:\d+)\s")


@dataclass(frozen=True)
class Moment:
    """One recording of the pack: a card's moment as one seat sees it."""

    recording: str
    seat: str


def b3d(arguments: list[str]) -> tuple[int, str]:
    """`just b3d <arguments>`: its exit status and its text (standard error included)."""
    done = subprocess.run(
        [*BY, "b3d", *arguments],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=False,
    )
    return done.returncode, done.stdout


def moments(slug: str, moment: str | None, seats: tuple[str, ...]) -> list[Moment]:
    """The recordings of `slug`'s spec (`<slug>/<moment>`, only `moment` when given) seen from
    each of `seats`, in the order `just b3d list` prints them."""
    code, text = b3d(["list", slug, "--json"])
    if code != 0:
        print(text, file=sys.stderr, end="")
        return []
    listed = json.loads(text.splitlines()[-1])["recordings"]
    prefix = f"{slug}/"
    return [
        Moment(r["name"], r["seat"])
        for r in listed
        if r["name"].startswith(prefix)
        and r["seat"] in seats
        and (moment is None or r["name"] in (moment, prefix + moment))
    ]


def cut(text: str, command: str) -> str:
    """`text` with at most `TREE_LINES` lines, and a notice of how many are left out."""
    lines = text.splitlines()
    if len(lines) <= TREE_LINES:
        return text.rstrip("\n")
    left = len(lines) - TREE_LINES
    notice = f"... {left} more lines (run `{command}` for all)"
    return "\n".join([*lines[:TREE_LINES], notice])


def show(command: list[str]) -> tuple[int, str]:
    """Runs one `just b3d` command, returning its status and its text under the line that
    reproduces it."""
    code, text = b3d(command)
    return code, text.rstrip("\n")


def describe(m: Moment, film: bool) -> tuple[list[str], int]:
    """The lines that describe one moment from one seat, and the worst exit status of the
    commands that made them."""
    seat = ["--seat", m.seat]
    worst = 0
    out = [f"## {m.recording} --seat {m.seat}"]
    code, info = show(["info", m.recording, *seat])
    worst = max(worst, code)
    out.append(cut(info, f"just b3d info {m.recording} --seat {m.seat}"))
    prompts = [p.group(1) for p in map(PROMPT_LINE.match, info.splitlines()) if p]
    for at in prompts or ["end"]:
        tree = ["tree", m.recording, *seat, "--at", at]
        code, text = show(tree)
        worst = max(worst, code)
        out.append(cut(text, "just b3d " + shlex.join(tree)))
    code, checked = show(["check", m.recording, *seat])
    worst = max(worst, code)
    # `check` prints the faults, or its summary line saying there are none.
    out.append(checked if code != 0 else f"no fault: {checked.splitlines()[-1]}")
    if film:
        code, pictured = show(
            ["film", m.recording, *seat, "--frames", str(FILM_FRAMES)]
        )
        worst = max(worst, code)
        out.append(pictured)
    return out, worst


def run_spec(args: argparse.Namespace) -> int:
    slugs = specs.resolve([args.card])
    if slugs is None:
        return 2
    seats = SEATS if args.seat == "both" else (args.seat,)
    found = moments(slugs[0], args.moment, seats)
    if not found:
        print(f"spec: no moments of {slugs[0]} to show", file=sys.stderr)
        return 2
    worst = 0
    for m in found:
        lines, code = describe(m, args.film)
        worst = max(worst, code)
        print("\n".join(lines), end="\n\n")
    return 1 if worst else 0


def spec_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("card", help="a card's name or slug")
    parser.add_argument("moment", nargs="?", help="one moment of the card's spec")
    parser.add_argument("--seat", choices=(*SEATS, "both"), default="both")
    parser.add_argument(
        "--film", action="store_true", help="also draw each moment as a film strip"
    )


COMMANDS: tuple[Command, ...] = (
    Command(
        "spec",
        "a card's spec moments from both seats: prompts, trees and checks",
        (
            ("just b3d spec counterspell", "every moment, from both seats"),
            (
                "just b3d spec counterspell counters-a-spell-into-its-owners-graveyard --seat you --film",
                "one moment from your seat, with its film",
            ),
        ),
        run_spec,
        spec_arguments,
    ),
)


def main_for(argv: list[str]) -> int:
    """Parses `argv` as `just b3d spec` does and runs it."""
    parser = argparse.ArgumentParser(prog="just b3d spec")
    spec_arguments(parser)
    return run_spec(parser.parse_args(argv))
