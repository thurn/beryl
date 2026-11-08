"""Bug reports from the app's bug button (beryl_session's `bugs` module writes them): `by bugs`
lists them, `by bugs file` files a bead for each open one, and `by bugs done` marks them fixed.

Each report is a folder under $BERYL_BUGS, else
`~/Library/Application Support/ca.thurn.beryl/bugs` (on Windows, `%APPDATA%\\ca.thurn.beryl\\bugs`):
`report.md`, whose front matter holds its
`status` (open, filed, done) and `bead`, beside its screenshots, replay, views and logs. A done
report moves to `done/`.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

from . import paths

REPORT = "report.md"
DONE = "done"
# Every bead filed from a report carries it, so `bd list --label bug-report` finds them.
LABEL = "bug-report"
# And a report of a questionable play of the AI's (`kind: ai`) carries this one too.
AI_LABEL = "ai-play"


def bugs_dir() -> Path:
    root = os.environ.get("BERYL_BUGS")
    if root:
        return Path(root)
    appdata = os.environ.get("APPDATA")
    if sys.platform == "win32" and appdata:
        return Path(appdata) / "ca.thurn.beryl" / "bugs"
    return Path.home() / "Library" / "Application Support" / "ca.thurn.beryl" / "bugs"


@dataclass
class Report:
    folder: Path
    fields: dict[str, str]
    body: str

    @property
    def id(self) -> str:
        return self.fields.get("id") or self.folder.name

    @property
    def status(self) -> str:
        return self.fields.get("status", "open")

    @property
    def kind(self) -> str:
        """`ai` for a questionable play of the AI's, else `bug`."""
        return self.fields.get("kind", "bug")

    @property
    def bead(self) -> str:
        return self.fields.get("bead", "")

    @property
    def title(self) -> str:
        title = self.fields.get("title", "")
        # Titles are written as JSON strings, which are YAML strings.
        if title.startswith('"') and title.endswith('"'):
            return str(json.loads(title))
        return title


def parse(text: str) -> tuple[dict[str, str], str]:
    """A report's front matter, as strings, and the rest."""
    lines = text.split("\n")
    if not lines or lines[0] != "---":
        return {}, text
    fields: dict[str, str] = {}
    for i, line in enumerate(lines[1:], start=1):
        if line == "---":
            return fields, "\n".join(lines[i + 1 :])
        key, _, value = line.partition(":")
        fields[key.strip()] = value.strip()
    return fields, ""


def load(folder: Path) -> Report | None:
    path = folder / REPORT
    if not path.is_file():
        return None
    fields, body = parse(path.read_text())
    return Report(folder, fields, body)


def update(report: Report, **changes: str) -> None:
    """Sets front matter fields of `report`, in place in its file."""
    path = report.folder / REPORT
    lines = path.read_text().split("\n")
    end = lines.index("---", 1)
    seen: set[str] = set()
    for i in range(1, end):
        key = lines[i].partition(":")[0].strip()
        if key in changes:
            lines[i] = f"{key}: {changes[key]}".rstrip()
            seen.add(key)
    missing = [f"{k}: {v}".rstrip() for k, v in changes.items() if k not in seen]
    lines[end:end] = missing
    path.write_text("\n".join(lines), newline="\n")
    report.fields.update(changes)


def reports(root: Path, done: bool = False) -> list[Report]:
    """The reports under `root`, oldest first; with `done`, the done ones too."""
    folders = sorted(p for p in root.iterdir() if p.is_dir()) if root.is_dir() else []
    found = [r for f in folders if f.name != DONE and (r := load(f))]
    if done and (root / DONE).is_dir():
        found += [r for f in sorted((root / DONE).iterdir()) if (r := load(f))]
    return found


def find(root: Path, key: str) -> Report | None:
    """The report with id `key` (or a unique prefix of it), or filed as bead `key`."""
    everything = reports(root, done=True)
    for r in everything:
        if key in (r.id, r.bead):
            return r
    matches = [r for r in everything if r.id.startswith(key)]
    return matches[0] if len(matches) == 1 else None


def bead_description(report: Report) -> str:
    f = report.fields
    game = ", ".join(
        f"{k} {f[k]}"
        for k in ("seed", "turn", "step", "decision", "session")
        if f.get(k)
    )
    ai = (
        "A questionable play of the AI's, for incident analysis: `report.md` digests "
        "the agent's latest searches as the game ran them, and `searches.json` holds "
        "them all. Write it up as an incident under `docs/incidents/`, as the ones "
        "there are, and fix what it finds.\n\n"
        if report.kind == "ai"
        else ""
    )
    return (
        ai + f"Reported from the app's bug button. The report, with its screenshot "
        f"(`screenshot.png`), the game's replay, its latest views and the logs:\n\n"
        f"`{report.folder}`\n\n"
        f"Read `{report.folder / REPORT}` first; the `bugfix` skill "
        f"(`.agents/bugfix/SKILL.md`) says how to reproduce and close it.\n\n"
        f"- Game: {game or 'none (not in a game)'}\n"
        f"- Built from: {f.get('commit', 'unknown')}\n"
        f"- Page: {f.get('url', '')}\n\n"
        f"## What the player said\n\n{description(report)}\n"
    )


def description(report: Report) -> str:
    """The player's words: the body's text under its title, up to the screenshot."""
    lines = report.body.strip().split("\n")
    if lines and lines[0].startswith("# "):
        lines = lines[1:]
    words: list[str] = []
    for line in lines:
        if line.startswith("![") or line.startswith("## "):
            break
        words.append(line)
    return "\n".join(words).strip()


def run_bd(args: list[str]) -> str:
    run = subprocess.run(
        ["bd", *args], cwd=paths.REPO, capture_output=True, text=True, check=False
    )
    if run.returncode != 0:
        raise RuntimeError(f"bd {' '.join(args[:2])}: {run.stderr.strip()}")
    return run.stdout.strip()


def file_bead(report: Report, priority: str) -> str:
    with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False) as body:
        body.write(bead_description(report))
    try:
        return run_bd(
            [
                "create",
                "--type",
                "bug",
                "--title",
                report.title or report.id,
                "--body-file",
                body.name,
                "--labels",
                f"{LABEL},{AI_LABEL}" if report.kind == "ai" else LABEL,
                "--priority",
                priority,
                "--external-ref",
                f"bug-report:{report.id}",
                "--silent",
            ]
        ).splitlines()[-1]
    finally:
        os.unlink(body.name)


def list_cmd(args: argparse.Namespace) -> int:
    root = bugs_dir()
    found = reports(root, done=args.all)
    if not found:
        print(f"bugs: no reports in {root}")
        return 0
    for r in found:
        bead = f" {r.bead}" if r.bead else ""
        kind = " [ai]" if r.kind == "ai" else ""
        print(f"{r.status:<6}{bead}{kind} {r.id}: {r.title}")
        print(f"       {r.folder}")
    return 0


def file_cmd(args: argparse.Namespace) -> int:
    root = bugs_dir()
    chosen = [find(root, k) for k in args.reports] if args.reports else None
    if chosen is not None and None in chosen:
        missing = [k for k, r in zip(args.reports, chosen) if r is None]
        print(f"bugs: no report {', '.join(missing)}", file=sys.stderr)
        return 1
    todo = [
        r
        for r in (chosen or reports(root))
        if r is not None and r.status == "open" and not r.bead
    ]
    if not todo:
        print("bugs: no open reports to file")
        return 0
    for r in todo:
        if args.dry_run:
            print(f"would file {r.id}: {r.title}")
            continue
        bead = file_bead(r, args.priority)
        update(r, status="filed", bead=bead)
        print(f"{bead} {r.id}: {r.title}")
    return 0


def done_cmd(args: argparse.Namespace) -> int:
    root = bugs_dir()
    code = 0
    for key in args.reports:
        r = find(root, key)
        if r is None:
            print(f"bugs: no report {key}", file=sys.stderr)
            code = 1
            continue
        changes = {"status": "done", "fixed": time.strftime("%Y-%m-%d")}
        if args.commit:
            changes["fix"] = args.commit
        update(r, **changes)
        if r.folder.parent.name != DONE:
            (root / DONE).mkdir(parents=True, exist_ok=True)
            r.folder.rename(root / DONE / r.folder.name)
        print(f"done {r.id}" + (f" ({r.bead})" if r.bead else ""))
    return code


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("bugs", help="the app's bug reports: list, file as beads, done")
    p.set_defaults(func=list_cmd, all=False)
    verbs = p.add_subparsers(dest="verb")
    v = verbs.add_parser("list", help="the open and filed reports")
    v.add_argument("--all", action="store_true", help="the done ones too")
    v.set_defaults(func=list_cmd)
    v = verbs.add_parser("file", help="file a bead for each open report")
    v.add_argument("reports", nargs="*", help="report ids (default: every open one)")
    v.add_argument("--priority", default="1", help="the beads' priority (0-4)")
    v.add_argument("--dry-run", action="store_true")
    v.set_defaults(func=file_cmd)
    v = verbs.add_parser("done", help="mark reports fixed and move them to done/")
    v.add_argument("reports", nargs="+", help="report ids or their beads")
    v.add_argument("--commit", help="the fixing commit")
    v.set_defaults(func=done_cmd)
