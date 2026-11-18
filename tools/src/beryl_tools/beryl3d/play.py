"""`just b3d play`: starts the windowless app on a fresh game and leaves it running, for a
reviewer who plays it through `just b3d live` (docs/beryl-3d.md, "Driving the running app").

`stand-in` starts the app and answers every prompt itself; `play` stops once the game is on
screen and prints how to go on, so a person-like player can read the table and act between
moves. The app ends by itself after `--for`.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

from .. import deps
from . import app, live, paths, stand_in
from .command import Command

# The longest a game may be left running: the app's `--for` is one bound on a hang.
LIMIT_S = 3600
# The opponent the start screen offers first, so it needs no `--offer` and no choosing.
DEFAULT_OPPONENT = "mcts-hidden"


def seconds(text: str) -> int:
    """`--for`'s value in seconds: a whole number with the unit `s`, `m` or `h`. Raises
    ValueError for anything else or a value of 0 or more than `LIMIT_S`."""
    units = {"s": 1, "m": 60, "h": 3600}
    number, unit = text[:-1], text[-1:]
    if unit not in units or not number.isdigit():
        raise ValueError(f"--for {text}: a duration is a whole number and s, m or h")
    total = int(number) * units[unit]
    if not 0 < total <= LIMIT_S:
        raise ValueError(f"--for {text}: at most {LIMIT_S} s, and not 0")
    return total


def follow_up(pid: int, socket: Path) -> str:
    """What the reviewer runs next, as printed when the game is on screen."""
    base = f"just b3d live %s --app {pid}"
    return "\n".join(
        [
            f"play: process id {pid}, socket {socket}",
            f"  read the table:  {base % 'tree'}",
            f"  draw it:         {base % 'png'}",
            f"  act:             {base % 'do'} '<action>'  (for example 'press button \"Keep\"')",
            f"  measure frames:  just b3d frames",
            f"  end the game:    {base % 'quit'}",
        ]
    )


def launch(opponent: str, run_for: int, log: Path) -> subprocess.Popen[bytes]:
    """Starts the app apart from this command: its output goes to `log`, and it is in a
    session of its own, so it keeps running when this command returns."""
    command = [str(app.BINARY), "--window", "none", "--for", f"{run_for}s"]
    if opponent != DEFAULT_OPPONENT:
        command += ["--offer", opponent]
    with log.open("wb") as output:
        return subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=output,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            env={**os.environ, "BERYL_PROFILE": f"b3d-play-{os.getpid()}"},
        )


def wait_log(log: Path, process: subprocess.Popen[bytes], prefix: str) -> list[str]:
    """The lines of `log` once one starts with `prefix`; raises StandInError when the app ends
    first or does not print it in time."""
    deadline = time.monotonic() + stand_in.START_LIMIT_S
    while time.monotonic() < deadline:
        lines = log.read_text(errors="replace").splitlines()
        if any(line.startswith(prefix) for line in lines):
            return lines
        if process.poll() is not None:
            break
        time.sleep(stand_in.POLL_S)
    raise stand_in.StandInError(
        f"the app never printed {prefix!r}; its output:\n"
        + log.read_text(errors="replace")
    )


def toggle_title(tree: stand_in.Tree, opponent: str) -> str:
    """The start screen's toggle for `opponent`, found by its name (`expert` is "Expert").
    Raises StandInError, listing the toggles, when none is named so."""
    for title in tree.named("toggle"):
        if title.casefold() == opponent.casefold():
            return title
    raise stand_in.StandInError(
        f"no opponent toggle is named {opponent!r}; the start screen has: "
        + ", ".join(tree.named("toggle"))
    )


def play_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--for",
        dest="run_for",
        help="how long the app runs before it ends itself (90s, 20m); required, at most 1h",
    )
    parser.add_argument(
        "--opponent",
        default=DEFAULT_OPPONENT,
        help=f"the agent to play against ({DEFAULT_OPPONENT})",
    )
    parser.add_argument("--no-build", action="store_true")


def run_play(args: argparse.Namespace) -> int:
    if args.run_for is None:
        print("play: --for is required, so that the app always ends", file=sys.stderr)
        return 2
    try:
        run_for = seconds(args.run_for)
    except ValueError as error:
        print(f"play: {error}", file=sys.stderr)
        return 2
    if not args.no_build and app.build() != 0:
        return deps.CANNOT_RUN
    out = paths.cache() / "out"
    out.mkdir(parents=True, exist_ok=True)
    log = out / f"play-{os.getpid()}.log"
    process = launch(args.opponent, run_for, log)
    try:
        lines = wait_log(log, process, app.FIRST_SHOWN)
        lines = wait_log(log, process, app.LIVE_SOCKET)
        running = stand_in.Running(process, lines=lines)  # type: ignore[arg-type]
        stand_in.wait_tree(running, "start")
        if args.opponent == DEFAULT_OPPONENT:
            running.act('press button "Start"')
        else:
            stand_in.pick_opponent(running, toggle_title(running.tree(), args.opponent))
        stand_in.wait_tree(running, "game")
        print(follow_up(process.pid, running.socket))
        print(f"play: the app's output goes to {log}")
    except stand_in.StandInError as error:
        print(f"play: FAILED: {error}", file=sys.stderr)
        process.kill()
        return 1
    return 0


COMMANDS: tuple[Command, ...] = (
    Command(
        "play",
        "starts the windowless app on a fresh game and leaves it running for `live`",
        (
            (
                "just b3d play --for 120s",
                "a game against mcts-hidden; prints the process id and the live commands",
            ),
            (
                "just b3d play --for 10m --opponent expert --no-build",
                "against expert, with the app as last built",
            ),
        ),
        run_play,
        play_arguments,
    ),
)
