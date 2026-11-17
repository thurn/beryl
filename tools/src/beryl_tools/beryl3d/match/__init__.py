"""`just b3d match <subcommand>`: the tools that find and measure every difference between
Beryl 3D's look and the web interface's at one moment (docs/beryl-3d.md, "Matching the web
interface's look"). Each subcommand is a module of this package with a `SUBCOMMAND`, listed in
`SUBCOMMANDS`; every one takes the same target (target.py) and reads the snapshot directory
that `snapshot` writes."""

from __future__ import annotations

import argparse

from ..command import Command
from . import boxes, motion, pixels, snapshot

SNAPSHOT = Command(
    "snapshot",
    "both clients' pictures and element records of one moment, in its snapshot directory",
    (
        (
            "just b3d match snapshot opening-play --at prompt:1",
            "the play-or-draw prompt, desktop, at scale 2",
        ),
        (
            "just b3d match snapshot b3d:opening-play/you/393x759 --at 100ms",
            "a phone window, 100 ms in",
        ),
    ),
    snapshot.run,
    snapshot.snapshot_arguments,
)

BOXES = Command(
    "boxes",
    "elements laid out differently on the two sides, those left unpaired, and text that "
    "overlaps on one side only",
    (
        (
            "just b3d match boxes opening-play --at prompt:1",
            "the play-or-draw prompt, desktop, at scale 2",
        ),
        (
            "just b3d match boxes --all",
            "every desktop and phone test target at its end, a total for each",
        ),
    ),
    boxes.run,
    boxes.box_arguments,
)

PIXELS = Command(
    "pixels",
    "the elements answerable for the pixels the two pictures differ in, worst first, with "
    "magnified crops of the worst",
    (
        (
            "just b3d match pixels opening-play --at prompt:1",
            "the play-or-draw prompt, desktop, at scale 2",
        ),
        (
            "just b3d match pixels opening-play --at prompt:1 --only 'button \"Play\"'",
            "one element's own error, with nothing behind it",
        ),
    ),
    pixels.run,
    pixels.pixel_arguments,
)

MOTION = Command(
    "motion",
    "each moving element's path, size and opacity on both sides, frame by frame over a span, "
    "with the largest differences and a plot of each",
    (
        (
            "just b3d match motion verbs-zones --from 1300ms --to 2000ms",
            "every element that moves in that span, desktop, at scale 2",
        ),
        (
            "just b3d match motion verbs-zones --from 1300ms --to 2000ms --layout phone "
            "--only 'card \"Plains\"'",
            "one card's tracks in the phone layout",
        ),
    ),
    motion.run,
    motion.motion_arguments,
)

# Each subcommand, in the order `just b3d match --help` lists them. A module adds one by writing
# its `Command` and adding it here.
SUBCOMMANDS: tuple[Command, ...] = (SNAPSHOT, BOXES, PIXELS, MOTION)


def arguments(parser: argparse.ArgumentParser) -> None:
    chosen = parser.add_subparsers(
        dest="subcommand", required=True, metavar="subcommand"
    )
    for sub in SUBCOMMANDS:
        width = max(len(line) for line, _ in sub.examples)
        examples = "\n".join(
            f"  {line:<{width}}  {what}" for line, what in sub.examples
        )
        made = chosen.add_parser(
            sub.name,
            help=sub.summary,
            description=sub.summary,
            epilog=f"examples:\n{examples}",
            formatter_class=argparse.RawDescriptionHelpFormatter,
        )
        sub.arguments(made)
        made.set_defaults(chosen_run=sub.run)


def run(args: argparse.Namespace) -> int:
    return args.chosen_run(args)


COMMANDS: tuple[Command, ...] = (
    Command(
        "match",
        "compares Beryl 3D's look with the web interface's at one moment (`match --help`)",
        SNAPSHOT.examples,
        run,
        arguments,
    ),
)
