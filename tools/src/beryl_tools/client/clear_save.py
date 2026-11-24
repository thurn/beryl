"""`just client clear-save`: sets the saved game aside, so that the next launch of `just client app`
(or the web app) opens on the start screen instead of resuming it.

On launch both apps resume the newest replay in the profile's `replays` folder when its game
has not ended (`Sessions::resume_latest` in `beryl_session`). The newest is the one whose file
name starts with the latest time (`newest_replay`), and only files ending in `.json` count. This
command moves that file into `replays/cleared/`, which the apps never read, so the game can be
put back by moving the file back.

A replay records the players' choices, not the result, so a game that ended by a win cannot be
told apart from one in progress without replaying it. A conceded game ends its log with
`"concede"`, and that one is left alone; any other newest game is moved aside.
"""

from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path

from .. import bugs
from .command import Command

# `Profile::parse` in `beryl_shell`: a portable folder name, lowercased.
PROFILE_NAME: re.Pattern[str] = re.compile(r"[A-Za-z0-9-]{1,255}")
DEVICE_NAMES: re.Pattern[str] = re.compile(r"(con|prn|aux|nul|com[1-9]|lpt[1-9])")
CLEARED: str = "cleared"


def data_dir(profile: str | None) -> Path:
    """The data folder of `profile` (`BERYL_PROFILE`), as `Dirs::desktop` finds it: the base
    folder, or `<base>/profiles/<name>`. An invalid name means the default profile, as it does
    for the apps."""
    base = bugs.app_data_dir()
    if profile and PROFILE_NAME.fullmatch(profile):
        name = profile.lower()
        if not DEVICE_NAMES.fullmatch(name):
            return base / "profiles" / name
    return base


def start_time(path: Path) -> tuple[int, int] | None:
    """The start time a replay's file name begins with, `<seconds>.<nanos>-<id>.json`, as
    `newest_replay` reads it; None for a file the apps would not resume."""
    if path.suffix != ".json":
        return None
    start, dash, _ = path.stem.partition("-")
    if not dash:
        return None
    seconds, _, nanos = start.partition(".")
    if not seconds.isdigit() or not (nanos or "0").isdigit():
        return None
    return int(seconds), int(nanos or "0")


def newest_replay(replays: Path) -> Path | None:
    timed = [
        (time, path)
        for path in replays.iterdir()
        if path.is_file() and (time := start_time(path)) is not None
    ]
    return max(timed)[1] if timed else None


def conceded(path: Path) -> bool:
    """Whether the replay's log ends with a concession. False when it cannot be read."""
    try:
        log = json.loads(path.read_text(encoding="utf-8")).get("log")
    except (OSError, ValueError, AttributeError):
        return False
    return isinstance(log, list) and bool(log) and log[-1] == "concede"


def clear(replays: Path) -> str:
    """Moves the newest replay into `replays/cleared/` unless it is a conceded game, and says
    what it did."""
    if not replays.is_dir():
        return f"clear-save: no saved games in {replays}"
    newest = newest_replay(replays)
    if newest is None:
        return f"clear-save: no saved games in {replays}"
    if conceded(newest):
        return (
            f"clear-save: the newest game was conceded, so none resumes ({newest.name})"
        )
    cleared = replays / CLEARED
    cleared.mkdir(exist_ok=True)
    moved = cleared / newest.name
    newest.rename(moved)
    return (
        f"clear-save: moved the saved game to {moved}\n"
        "  The next launch opens on the start screen. To resume it instead, move it back:\n"
        f'  mv "{moved}" "{replays}/"'
    )


def run_clear_save(args: argparse.Namespace) -> int:
    del args
    print(clear(data_dir(os.environ.get("BERYL_PROFILE")) / "replays"))
    return 0


COMMANDS: tuple[Command, ...] = (
    Command(
        "clear-save",
        "sets the saved game aside, so that the app opens on the start screen",
        (
            ("just client clear-save", "the next `just client app` starts a new game"),
            (
                "BERYL_PROFILE=seat-1 just client clear-save",
                "the same for the profile seat-1",
            ),
        ),
        run_clear_save,
    ),
)
