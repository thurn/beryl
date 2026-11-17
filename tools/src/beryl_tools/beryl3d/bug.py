"""`just b3d bug <report>`: what a worker fixing a Beryl 3D bug report reads first, and the
commands that bring the app back to the reported state (docs/beryl-3d.md, 'Bug reports').

A report is a folder that beryl_session's `bugs` module wrote (`tools/src/beryl_tools/bugs.py`
reads them). Beryl 3D's report names the client in its `ui.json` log and holds `tree.txt`, the
element tree of the frozen frame, and `frames.jsonl`, the frame log so far. A report saved
while a recording replayed also records the recording, its seat and the moment (`at`, in
milliseconds, and whether the replay had already ended) in `ui.json`, so `just b3d tree` reaches the same frame with no game. A report
from a live game holds the game's `replay.json`, which `just b3d app --replay` resumes.

The command writes no file.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

from .. import bugs
from . import app
from .command import Command

# How many lines of `tree.txt` are printed.
TREE_LINES = 60
CLIENT = "Beryl 3D"


@dataclass(frozen=True)
class Opened:
    report: bugs.Report
    # The `log` of `ui.json`: a mapping in Beryl 3D's report, a list of entries in the web's.
    log: object


def open_report(key: str, root: Path) -> Opened | None:
    """The report at the folder `key`, or the one `bugs.find` finds by `key` under `root`."""
    folder = Path(key).expanduser()
    report = bugs.load(folder) if folder.is_dir() else bugs.find(root, key)
    if report is None:
        return None
    log: object = None
    ui = report.folder / "ui.json"
    if ui.is_file():
        log = json.loads(ui.read_text(encoding="utf-8")).get("log")
    return Opened(report, log)


def is_beryl3d(log: object) -> bool:
    return isinstance(log, dict) and log.get("client") == CLIENT


def reproduce(opened: Opened) -> list[str]:
    """The commands that show the reported state again: the replayed recording's moment, or
    the live game resumed from its replay; none when the report has neither."""
    log = opened.log
    if isinstance(log, dict) and "recording" in log and "at" in log:
        # A report saved after the replay's last script ended shows its last moment.
        at = "end" if log.get("ended") else f"{log['at']}ms"
        moment = f"{log['recording']} --seat {log.get('seat', 'you')} --at {at}"
        return [f"just b3d tree {moment}", f"just b3d png {moment}"]
    replay = opened.report.folder / "replay.json"
    if replay.is_file():
        return [
            f"just b3d app --replay {replay} --window none --for 120s",
            "just b3d live tree",
        ]
    return []


def text(opened: Opened) -> str:
    """Everything `just b3d bug` prints for `opened` after the command line, which `cli.main`
    prints."""
    report = opened.report
    out = [f"report: {report.folder}"]
    # A description short enough to be the title is written only as the title.
    words = bugs.description(report) or report.title or "(no description)"
    if not is_beryl3d(opened.log):
        out += [
            "a web report: its ui.json does not name Beryl 3D. Follow the bugfix skill's web",
            "steps ('Worker: fixing one report' in .claude/skills/bugfix/SKILL.md).",
            "",
            words,
        ]
        return "\n".join(out)
    out += [
        f"client: {CLIENT}, built from {report.fields.get('commit') or 'unknown'}",
        "",
        words,
        "",
    ]
    tree = report.folder / "tree.txt"
    if tree.is_file():
        lines = tree.read_text(encoding="utf-8").splitlines()
        shown = (
            f"first {TREE_LINES} of {len(lines)}" if len(lines) > TREE_LINES else "all"
        )
        out += [f"tree.txt ({shown} lines):", *lines[:TREE_LINES], ""]
    else:
        out += ["no tree.txt", ""]
    frames = report.folder / "frames.jsonl"
    if frames.is_file():
        out += ["frames.jsonl:", app.report(frames), ""]
    commands = reproduce(opened)
    if commands:
        out += ["reproduce:", *(f"  {command}" for command in commands)]
    else:
        out.append("reproduce: the report has no recording moment and no replay.json")
    return "\n".join(out)


def bug_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "report", help="a report folder, or an id, id prefix or bead of one"
    )


def run_bug(args: argparse.Namespace) -> int:
    key: str = args.report
    root = bugs.bugs_dir()
    opened = open_report(key, root)
    if opened is None:
        print(f"bug: no report {key!r} in {root}", file=sys.stderr)
        return 1
    print(text(opened))
    return 0


COMMANDS: tuple[Command, ...] = (
    Command(
        "bug",
        "a bug report: its words, element tree, frame log and the commands that reproduce it",
        (
            ("just b3d bug by-abcd", "the report filed as bead by-abcd"),
            (
                "just b3d bug ~/Library/Application\\ Support/ca.thurn.beryl/bugs/<folder>",
                "one report folder",
            ),
        ),
        run_bug,
        bug_arguments,
    ),
)
