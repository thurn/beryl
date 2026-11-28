"""`just client bug <report>`: what a worker fixing a the client bug report reads first, and the
commands that bring the app back to the reported state (docs/beryl-3d.md, 'Bug reports').

A report is a folder that beryl_session's `bugs` module wrote (`tools/src/beryl_tools/bugs.py`
reads them). The client's report names the client in its `ui.json` log and holds `tree.txt`, the
element tree of the frozen frame, and `frames.jsonl`, the frame log so far. A report saved
while a recording replayed also records the recording, its seat and the moment (`at`, in
milliseconds, and whether the replay had already ended) in `ui.json`, so `just client tree`
reaches the same frame with no game. A report whose folder is not on this machine is downloaded from its ticket's attachments into
`$BERYL_CACHE/bug-reports/<ticket>`. A report from a live game holds the game's `replay.json`, which `just client app --replay` resumes.

Without options the command writes no file. `--png`, `--film` and `--story` draw the reported
moment on the current tree and write it as a story (`bug_moment`).
"""

from __future__ import annotations

import argparse
import gzip
import json
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

from .. import bugs, mobile_bugs, paths
from . import app, bug_moment, phone
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


def download_report(ticket: str) -> Path | None:
    """The folder of `ticket`'s attached capture under `$BERYL_CACHE/bug-reports/<ticket>`,
    downloaded when it is not there yet, or None when the ticket has none.

    A file is downloaded again only when its `sha256` on the ticket differs from the one
    recorded in the folder's `.sha256.json`. `frames.jsonl.gz` is unpacked to `frames.jsonl`.
    """
    wanted = {*bugs.CAPTURE_FILES, bugs.FRAMES_GZ, *(name for name, _ in bugs.IMAGES)}
    try:
        listed = bugs.ignite_ticket("attachment", "list", ticket)
    except (OSError, bugs.TicketError):
        return None
    attached = {
        str(item["name"]): str(item.get("sha256", ""))
        for item in listed.get("attachments") or []
        if not item.get("removed_at") and item.get("name") in wanted
    }
    if bugs.REPORT not in attached:
        return None
    folder = paths.cache_root() / "bug-reports" / ticket
    folder.mkdir(parents=True, exist_ok=True)
    record = folder / ".sha256.json"
    try:
        known = json.loads(record.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        known = {}
    for name, sha in attached.items():
        target = folder / name
        if known.get(name) == sha and target.is_file():
            continue
        try:
            bugs.ignite_ticket("attachment", "get", ticket, name, "-o", str(target))
        except (OSError, bugs.TicketError) as error:
            print(f"bug: {error}", file=sys.stderr)
            continue
        if name == bugs.FRAMES_GZ:
            with (
                gzip.open(target, "rb") as source,
                (folder / bugs.FRAMES).open("wb") as unpacked,
            ):
                shutil.copyfileobj(source, unpacked)
        known[name] = sha
    record.write_text(json.dumps(known), encoding="utf-8")
    return folder


def open_report(key: str, root: Path) -> Opened | None:
    """The report at the folder `key`, or the one `bugs.find` finds by `key` under `root`, or
    else the one attached to the ticket `key`."""
    folder = Path(key).expanduser()
    report = bugs.load(folder) if folder.is_dir() else bugs.find(root, key)
    if report is None and not folder.is_dir():
        downloaded = download_report(key)
        report = bugs.load(downloaded) if downloaded else None
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
    table = opened.report.folder / "table.jsonl"
    if table.is_file() and opened.report.fields.get("table") == "solo":
        return [
            f"just client app --table {table} --window none --for 120s",
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
    elif table_kind := report.fields.get("table"):
        details = [
            part
            for part in (
                f"deck {report.fields['deck']}" if report.fields.get("deck") else None,
                f"turn {report.fields['turn']}" if report.fields.get("turn") else None,
                f"step {report.fields['step']}" if report.fields.get("step") else None,
            )
            if part
        ]
        suffix = f" ({', '.join(details)})" if details else ""
        out.append(
            f"reproduce: no replay exists for this {table_kind} manual table{suffix}; "
            f"rebuild it from the start screen by opening a {table_kind} table"
        )
    else:
        out.append("reproduce: the report has no recording moment and no replay.json")
    return "\n".join(out)


def bug_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "report", help="a report folder, or an id, id prefix or ticket of one"
    )
    bug_moment.arguments(parser)


def run_bug(args: argparse.Namespace) -> int:
    key: str = args.report
    root = bugs.bugs_dir()
    opened = open_report(key, root)
    if opened is None:
        print(f"bug: no report {key!r} in {root}", file=sys.stderr)
        return 1
    if not (args.png or args.film is not None or args.story is not None):
        print(text(opened))
        return 0
    if not is_client(opened.log):
        print(text(opened))
        return 1
    words = bugs.description(opened.report) or opened.report.title or ""
    print(f"report: {opened.report.folder}\n{words}\n")
    return bug_moment.run(opened, args, words)


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
        "a bug report: its words, tree and frame log; --png, --film and --story draw its moment",
        (
            (
                "just client bug by-abcd --png --film 1500",
                "the report filed as ticket by-abcd, its moment drawn and filmed",
            ),
            (
                "just client bug by-abcd --do 'press pile \"Your graveyard\"' --png --story graveyard-zoom",
                "open a viewer first, and write the moment as a story",
            ),
        ),
        run_bug,
        bug_arguments,
    ),
)
