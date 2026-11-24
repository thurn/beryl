"""`just client bug <report>`: what a worker fixing a the client bug report reads first, and the
commands that bring the app back to the reported state (docs/beryl-3d.md, 'Bug reports').

A report is a folder that beryl_session's `bugs` module wrote (`tools/src/beryl_tools/bugs.py`
reads them). The client's report names the client in its `ui.json` log and holds `tree.txt`, the
element tree of the frozen frame, and `frames.jsonl`, the frame log so far. A report saved
while a recording replayed also records the recording, its seat and the moment (`at`, in
milliseconds, and whether the replay had already ended) in `ui.json`, so `just client tree`
reaches the same frame with no game. A report from a live game holds the game's `replay.json`, which `just client app --replay` resumes.

The command writes no file.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

from .. import bugs, mobile_bugs
from . import app, phone
from .command import Command

# How many lines of `tree.txt` are printed.
TREE_LINES = 60
CLIENT = "Beryl"
# The names the client's reports have given it: reports saved before the client was renamed
# from Beryl 3D name it so.
CLIENT_NAMES: tuple[str, ...] = (CLIENT, "Beryl 3D")


@dataclass(frozen=True)
class Opened:
    report: bugs.Report
    # The `log` of `ui.json`: a mapping in The client's report, a list of entries in the web's.
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


def is_client(log: object) -> bool:
    return isinstance(log, dict) and log.get("client") in CLIENT_NAMES


def reproduce(opened: Opened) -> list[str]:
    """The commands that show the reported state again: the replayed recording's moment, or
    the live game resumed from its replay; none when the report has neither."""
    log = opened.log
    if isinstance(log, dict) and "recording" in log and "at" in log:
        # A report saved after the replay's last script ended shows its last moment.
        at = "end" if log.get("ended") else f"{log['at']}ms"
        moment = f"{log['recording']} --seat {log.get('seat', 'you')} --at {at}"
        return [f"just client tree {moment}", f"just client png {moment}"]
    replay = opened.report.folder / "replay.json"
    if replay.is_file():
        return [
            f"just client app --replay {replay} --window none --for 120s",
            # `just client app` runs until `--for` ends, so `live` is run beside it.
            "just client live tree  (from another shell while the app runs)",
        ]
    return []


def text(opened: Opened) -> str:
    """Everything `just client bug` prints for `opened` after the command line, which `cli.main`
    prints."""
    report = opened.report
    out = [f"report: {report.folder}"]
    # A description short enough to be the title is written only as the title.
    words = bugs.description(report) or report.title or "(no description)"
    if not is_client(opened.log):
        out += [
            "a web report: its ui.json does not name the client. Follow the bugfix skill's web",
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
        "report", help="a report folder, or an id, id prefix or ticket of one"
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


IOS_REPORTS = Path("Documents/bugs")
ANDROID_REPORTS = "files/bugs"


def android_reports(root: Path) -> mobile_bugs.ReportCounts | None:
    if phone.adb("get-state").stdout.strip() != "device":
        return None
    listed = phone.adb(
        "shell", "run-as", phone.BUNDLE, "find", ".", "-type", "d", "-name", "bugs"
    )
    if listed.returncode != 0:
        return None
    if f"./{ANDROID_REPORTS}" not in listed.stdout.splitlines():
        return 0, 0
    return mobile_bugs.pull_android_archive(
        root, str(phone.adb_binary()), phone.SERIAL, phone.BUNDLE, ANDROID_REPORTS
    )


def run_mobile_bugs(args: argparse.Namespace) -> int:
    return mobile_bugs.pull(
        args,
        lambda root: mobile_bugs.simulator_reports(root, phone.BUNDLE, IOS_REPORTS),
        android_reports,
        lambda root: mobile_bugs.device_reports(root, phone.BUNDLE, IOS_REPORTS),
    )


COMMANDS: tuple[Command, ...] = (
    Command(
        "mobile-bugs",
        "pulls bug reports from native phone hosts or imports a shared ZIP",
        (
            (
                "just client mobile-bugs",
                "pull reports from a booted simulator and emulator",
            ),
            (
                "just client mobile-bugs --device",
                "also pull reports from a paired iPhone",
            ),
        ),
        run_mobile_bugs,
        mobile_bugs.arguments,
    ),
    Command(
        "bug",
        "a bug report: its words, element tree, frame log and the commands that reproduce it",
        (
            ("just client bug by-abcd", "the report filed as ticket by-abcd"),
            (
                "just client bug ~/Library/Application\\ Support/ca.thurn.beryl/bugs/<folder>",
                "one report folder",
            ),
        ),
        run_bug,
        bug_arguments,
    ),
)
