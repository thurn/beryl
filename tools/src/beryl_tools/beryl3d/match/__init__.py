"""`just b3d match <subcommand>`: the tools that find and measure every difference between
Beryl 3D's look and the web interface's at one moment (docs/beryl-3d.md, "Matching the web
interface's look"). Each subcommand is a module of this package with a `SUBCOMMAND`, listed in
`SUBCOMMANDS`; every one takes the same target (target.py) and reads the snapshot directory
that `snapshot` writes."""

from __future__ import annotations

import argparse

from ..command import Command
from . import snapshot

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

# Each subcommand, in the order `just b3d match --help` lists them. A module adds one by writing
# its `Command` and adding it here.
SUBCOMMANDS: tuple[Command, ...] = (SNAPSHOT,)


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
