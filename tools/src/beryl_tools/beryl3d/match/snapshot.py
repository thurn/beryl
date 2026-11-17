"""`just b3d match snapshot`: both clients' pictures and element records of one moment, in the
target's snapshot directory (target.py), which the other `match` subcommands read.

1. `b3d tree --records` plays the recording to the moment in Beryl 3D and writes `b3d.json`:
   every element with its box, paint order, style in CSS's names and lines of text. It also
   says which view the moment falls in, when that view arrived and whether the table is still
   moving.
2. `b3d-render png` draws the moment at the target's scale as `b3d.png`.
3. The recording is exported as the web interface's fixture (`b3d export --only`), and
   dump.mjs opens the page `?fixture=<name>&at=<n>#/game` in WebKit on Playwright's paused
   clock. A moment in the middle of an animation runs the clock as long after the view's
   arrival as Beryl 3D's moment lies; a moment at rest runs it until the stage is idle. It
   writes `web.png` and `web.json`, the same records read from the DOM and its computed styles.
4. `report.txt` names the moment and the files, and warns of any input the two sides do not
   share: the fonts (fonts.py) and the card scans.

Card faces: by default neither side draws a scan. A fixture page draws placeholder faces, and
Beryl 3D, given an empty scan directory (`BERYL3D_SCANS`), draws Beryl's card frame. With
`--art` both read the app's scan cache: Beryl 3D directly, and the page through a route that
serves Scryfall's image paths from it.

A snapshot is reused while its inputs (the target, `--art`, and the sources of both clients)
are unchanged, unless `--fresh`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from ... import paths as repo_paths
from ... import shots
from .. import compare
from . import fonts, records
from .target import Target, arguments, target

HERE: Path = Path(__file__).resolve().parent
ROLES: Path = repo_paths.REPO / "data" / "beryl3d" / "match" / "roles.json"
# The files a snapshot directory holds once this command has run.
FILES = ("web.png", "web.json", "b3d.png", "b3d.json", "report.txt")
# What a change to either client's look or to these tools is made in.
SOURCES = (
    "ui",
    "rust/beryl3d",
    "rust/crates",
    "data/beryl3d",
    "data/design-tokens.json",
    "tools/src/beryl_tools/beryl3d/match",
)
# The app's scan cache, which Beryl 3D's `Scans::app` reads on macOS.
APP_SCANS: Path = Path.home() / "Library" / "Caches" / "ca.thurn.beryl"
# `b3d-render png`'s line naming the file it wrote.
WROTE: re.Pattern[str] = re.compile(r"^wrote (?P<path>\S+\.png)", re.MULTILINE)


def snapshot_arguments(parser: argparse.ArgumentParser) -> None:
    arguments(parser)
    parser.add_argument(
        "--fresh", action="store_true", help="take it again even if nothing changed"
    )
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )


def inputs(t: Target, art: bool) -> str:
    """A digest of what the snapshot of `t` depends on: the target, `--art`, the commit and any
    uncommitted change to the sources of either client or these tools."""
    git = ["git", "-C", str(repo_paths.REPO)]
    head = subprocess.run(
        [*git, "rev-parse", "HEAD"], capture_output=True, text=True, check=False
    ).stdout
    diff = subprocess.run(
        [*git, "diff", "HEAD", "--", *SOURCES],
        capture_output=True,
        text=True,
        check=False,
    ).stdout
    digest = hashlib.sha256()
    for part in (repr(t), str(art), head, diff):
        digest.update(part.encode())
        digest.update(b"\0")
    return digest.hexdigest()


def render_png(t: Target, art: bool, out: Path) -> str | None:
    """Draws the moment with `b3d-render png` and copies it to `out`; None when it worked, else
    why not. Without `art` it reads scans from an empty directory, so every face is drawn.
    """
    from .. import cli

    built = cli.quick_build(
        ["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"]
    )
    if built != 0:
        return "b3d-render did not build"
    with tempfile.TemporaryDirectory(prefix="b3d-match-scans-") as empty:
        env = {**os.environ}
        if not art:
            env["BERYL3D_SCANS"] = empty
        line = [str(cli.linked("b3d-render")), "png", t.recording, *t.common()]
        line += ["--at", t.at]
        done = subprocess.run(
            line, capture_output=True, text=True, env=env, check=False
        )
        if done.returncode == cli.MISSING_PACK and cli.record() == 0:
            done = subprocess.run(
                line, capture_output=True, text=True, env=env, check=False
            )
    wrote = WROTE.search(done.stdout)
    if done.returncode != 0 or not wrote:
        return f"b3d-render png failed:\n{done.stdout}{done.stderr}"
    shutil.move(wrote["path"], out)
    return None


def dump_web(
    t: Target,
    messages: list[dict[str, object]],
    moment: dict[str, object],
    art: bool,
    out: Path,
) -> tuple[list[str], str | None]:
    """Shoots the web interface's page of `messages` at `moment` (`b3d.json`'s) into `out`'s
    `web.png` and `web.json`. Returns the lines dump.mjs printed (page errors, scans missing)
    and, if it failed, why."""
    width, height = t.width_height()
    name = f"b3d-match-{os.getpid()}"
    fixture = shots.RECORDED / f"{name}.json"
    fixture.parent.mkdir(parents=True, exist_ok=True)
    fixture.write_text(json.dumps({"messages": messages}))
    phone = "phone&" if t.layout == "phone" else ""
    at = compare.web_at(messages, int(str(moment["view"])))
    given = moment.get("view_given_ms")
    offset = None
    if moment.get("animating") and isinstance(given, int):
        offset = int(str(moment["t_ms"])) - given
    job = {
        "route": f"/?{phone}fixture={name}&at={at}#/game",
        "width": width,
        "height": height,
        "scale": float(t.scale),
        "offset_ms": offset,
        "png": str(out / "web.png"),
        "json": str(out / "web.json"),
        "roles": json.loads(ROLES.read_text())["roles"],
        "target": t.test_name(),
        "art_dir": str(APP_SCANS / "art") if art else None,
    }
    job_file = out / "web-job.json"
    try:
        env = shots.browser_env()
        with shots.served_ui(env) as url:
            job_file.write_text(json.dumps({**job, "base": url}))
            done = subprocess.run(
                ["node", str(HERE / "dump.mjs"), str(job_file)],
                cwd=repo_paths.UI,
                env=env,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                check=False,
            )
    except shots.UiError as error:
        return [], str(error)
    finally:
        fixture.unlink(missing_ok=True)
        job_file.unlink(missing_ok=True)
    lines = done.stdout.splitlines()
    if done.returncode != 0:
        return lines, f"dump.mjs failed:\n{done.stdout}{done.stderr}"
    return lines, None


def report(t: Target, moment: dict[str, object], warnings: list[str], out: Path) -> str:
    """The snapshot's report: how to take it again, the moment, the warnings and the files."""
    web = json.loads((out / "web.json").read_text())
    b3d = json.loads((out / "b3d.json").read_text())
    state = "at rest"
    given = moment.get("view_given_ms")
    if moment.get("animating") and isinstance(given, int):
        # dump_web runs the page's clock this long after the view arrives.
        state = (
            f"animating, {int(str(moment['t_ms'])) - given}ms after the view arrived"
        )
    command = f"just b3d match snapshot {t.recording} --at {t.at} " + " ".join(
        t.common()
    )
    lines = [
        command,
        f"target {t.test_name()} ({t.layout}), view {moment['view']} of {moment['views']}, "
        f"t={moment['t_ms']}ms, {state}",
        f"web: {len(web['elements'])} elements, Beryl 3D: {len(b3d['elements'])} elements",
        *warnings,
        *(str(out / name) for name in FILES),
    ]
    return "\n".join(lines) + "\n"


def ensure(t: Target, fresh: bool = False, art: bool = False) -> Path | None:
    """The snapshot directory of `t`, taken now unless an up-to-date one exists; None after
    printing why it could not be taken. The other `match` subcommands start from this.
    """
    out = t.directory()
    digest = inputs(t, art)
    stamp = out / "inputs.txt"
    whole = all((out / name).exists() for name in FILES)
    if not fresh and whole and stamp.exists() and stamp.read_text() == digest:
        return out
    stamp.unlink(missing_ok=True)

    printed = compare.b3d(["tree", t.recording, *t.common(), "--at", t.at, "--records"])
    if printed is None:
        return None
    printout = json.loads(printed.strip().splitlines()[-1])
    moment = {
        key: printout[key]
        for key in ("view", "views", "t_ms", "view_given_ms", "animating")
    }
    (out / "b3d.json").write_text(
        json.dumps(
            {
                "target": t.test_name(),
                "scale": float(t.scale),
                "moment": moment,
                "elements": printout["elements"],
            },
            indent=1,
        )
        + "\n"
    )
    failed = render_png(t, art, out / "b3d.png")
    if failed:
        print(f"match snapshot: {failed}", file=sys.stderr)
        return None

    work = Path(tempfile.mkdtemp(prefix="b3d-match-"))
    try:
        if compare.b3d(["export", str(work), "--only", t.recording]) is None:
            return None
        exported = work / f"{t.recording.replace('/', '__')}.{t.seat}.json"
        messages = json.loads(exported.read_text())
    finally:
        shutil.rmtree(work, ignore_errors=True)
    printed_lines, failed = dump_web(t, messages, moment, art, out)
    if failed:
        print(f"match snapshot: {failed}", file=sys.stderr)
        return None

    for side in ("web", "b3d"):
        broken = records.faults(json.loads((out / f"{side}.json").read_text()))
        if broken:
            listed = "\n  ".join(broken[:10])
            print(
                f"match snapshot: {side}.json breaks the record contract:\n  {listed}",
                file=sys.stderr,
            )
            return None
    warnings = fonts.check()
    missing = [line for line in printed_lines if line.startswith("scan missing:")]
    if art:
        if missing:
            warnings.append(
                f"warning: scans: {len(missing)} card images are missing from the app's scan "
                f"cache ({APP_SCANS / 'art'}): the web shows none and Beryl 3D its card frame"
            )
    else:
        warnings.append(
            "note: scans: neither side draws card scans (the web its placeholder faces, Beryl "
            "3D its card frame); --art draws the app's scan cache on both"
        )
    warnings += [
        f"warning: web {line}" for line in printed_lines if line not in missing
    ]
    (out / "report.txt").write_text(report(t, moment, warnings, out))
    stamp.write_text(digest)
    return out


def run(args: argparse.Namespace) -> int:
    t = target(args)
    out = ensure(t, fresh=args.fresh, art=args.art)
    if out is None:
        return 1
    print((out / "report.txt").read_text(), end="")
    return 0
