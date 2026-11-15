"""`just b3d`: Beryl 3D's commands. With no argument it lists them, one line each;
`just b3d <command> --help` prints a command's options and two examples.

Every command follows the rules in docs/beryl-3d.md ('The agent interface'): its output begins
with the command that reproduces it, is bounded, and names any file it writes for the run.
"""

from __future__ import annotations

import argparse
import difflib
import shlex
import sys
from collections.abc import Callable
from dataclasses import dataclass

from . import lane

# Exit code for a command line `just b3d` cannot run, as argparse uses.
USAGE = 2


@dataclass(frozen=True)
class Command:
    name: str
    # The one line `just b3d` lists it with.
    summary: str
    # Two command lines, each with what it does, for `--help`.
    examples: tuple[tuple[str, str], tuple[str, str]]
    run: Callable[[argparse.Namespace], int]
    # Adds the command's own arguments to its parser.
    arguments: Callable[[argparse.ArgumentParser], None] = lambda _parser: None


def run_lane(_args: argparse.Namespace) -> int:
    from .. import checks  # It imports this package's lane.

    return checks.run_all([lane.lane()], "b3d", checks.FAST_BUDGET).code


COMMANDS: tuple[Command, ...] = (
    Command(
        "lane",
        "runs the beryl3d lane's steps, as the fast check does for a Beryl 3D change",
        (
            ("just b3d lane", "every step of the lane"),
            (
                "CHECK_HANG_LIMIT_S=600 just b3d lane",
                "every step, each allowed 600 s before it counts as hung",
            ),
        ),
        run_lane,
    ),
)


def listing() -> str:
    width = max(len(c.name) for c in COMMANDS)
    lines = ["just b3d", *(f"  {c.name:<{width}}  {c.summary}" for c in COMMANDS)]
    lines.append("`just b3d <command> --help` prints a command's options and examples.")
    return "\n".join(lines)


def parser(command: Command) -> argparse.ArgumentParser:
    width = max(len(line) for line, _ in command.examples)
    examples = "\n".join(
        f"  {line:<{width}}  {what}" for line, what in command.examples
    )
    made = argparse.ArgumentParser(
        prog=f"just b3d {command.name}",
        description=command.summary,
        epilog=f"examples:\n{examples}",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    command.arguments(made)
    return made


def main(argv: list[str]) -> int:
    """Runs `just b3d` with the arguments after `b3d`."""
    if not argv or argv[0] in ("-h", "--help"):
        print(listing())
        return 0
    found = {c.name: c for c in COMMANDS}
    command = found.get(argv[0])
    if command is None:
        nearest = difflib.get_close_matches(argv[0], list(found), n=1, cutoff=0.0)
        print(
            f"just b3d: no command {argv[0]!r}; the nearest is {nearest[0]!r}."
            " `just b3d` lists them.",
            file=sys.stderr,
        )
        return USAGE
    args = parser(command).parse_args(argv[1:])
    print(shlex.join(["just", "b3d", *argv]), flush=True)
    return command.run(args)


def b3d(args: argparse.Namespace) -> int:
    return main(list(args.extra))


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    # Every argument after `b3d` is this package's to parse, `--help` among them.
    p = sub.add_parser("b3d", help="Beryl 3D's commands", add_help=False)
    p.set_defaults(func=b3d, passthrough=True)
