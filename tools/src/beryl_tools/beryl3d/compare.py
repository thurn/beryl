"""`just b3d compare`: one PNG with the web interface's screenshot of a recording's moment on the
left and Beryl 3D's frame of the same moment on the right.

Beryl 3D keeps the web interface's look, and no check compares whole pictures (decision C5 in
docs/beryl-3d.md), so a session judges a look by reading the two side by side. This command:

1. asks `b3d tree` which view of the recording the moment falls in (`view 3 of 7`);
2. exports the recording with `b3d export --only`, the fixture file the web interface reads, and
   serves it from `ui/public/fixtures` under a name of this run's own;
3. shoots the page `?fixture=<name>&at=<n>#/game` at the same window size, as `just ui-shots`
   shoots a shot (`ui/scripts/shots.mjs`), where `at` releases the events up to that view, and
   keeps the screenshot under `$BERYL_CACHE/b3d/out`, so that `--web <png>` can reuse it;
4. runs `b3d-render compare` with that screenshot as `--web`, which draws the moment and writes
   both pictures into one file under `$BERYL_CACHE/b3d/out`.

The web page shows the view once it is at rest, so for a moment in the middle of an animation
(`--at 1500ms`) the two sides differ by the animation's progress.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from types import ModuleType

from .. import paths as repo_paths
from .. import shots
from .command import Command

USAGE = 2

# `b3d tree`'s line saying which view of the recording the frame shows.
VIEW_LINE: re.Pattern[str] = re.compile(r"^view (\d+) of \d+", re.MULTILINE)
# A test's name, as `just b3d test` prints it: b3d:<recording>/<seat>/<width>x<height>.
TEST_NAME: re.Pattern[str] = re.compile(
    r"^b3d:(?P<name>.+)/(?P<seat>you|opponent)/(?P<size>\d+x\d+)$"
)
# The window each layout's pictures use when `--size` is not given (`beryl_inspect`'s options).
DEFAULT_SIZE = {"desktop": "1600x900", "phone": "393x759"}
# The same moment as a file name part, as `b3d-render` makes one.
UNSAFE: re.Pattern[str] = re.compile(r"[^A-Za-z0-9.-]")


def arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "recording", help="a recording `just b3d list` names, or a test's name"
    )
    parser.add_argument("--at", required=True, help="the moment, as `png` takes it")
    parser.add_argument("--seat", choices=("you", "opponent"), default=None)
    parser.add_argument("--layout", choices=("desktop", "phone"), default="desktop")
    parser.add_argument("--size", default=None, help="<width>x<height> in points")
    parser.add_argument(
        "--scale",
        default="1",
        help="pixels a point, for both pictures (the desktop app on a Retina Mac uses 2)",
    )
    parser.add_argument(
        "--web",
        default=None,
        help="a web screenshot taken before, to use instead of shooting the page again",
    )


def target(args: argparse.Namespace) -> tuple[str, str, str]:
    """The recording, seat and window size `args` name, a test's name taking the seat and size
    from it."""
    found = TEST_NAME.match(args.recording)
    if found:
        return found["name"], args.seat or found["seat"], args.size or found["size"]
    return args.recording, args.seat or "you", args.size or DEFAULT_SIZE[args.layout]


def b3d(arguments: list[str]) -> str | None:
    """The output of the `b3d` program's text command `arguments`, or None after printing why
    it failed. It builds `b3d` as `just b3d tree` does, and the recordings pack if missing.
    """
    from . import cli

    if cli.quick_build(["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]) != 0:
        return None
    line = [str(cli.linked("b3d")), *arguments]
    done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode == cli.MISSING_PACK and cli.record() == 0:
        done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        sys.stderr.write(done.stdout + done.stderr)
        return None
    return done.stdout


def web_at(messages: list[dict[str, object]], view: int) -> int:
    """The `at` of a fixture route that opens the page at its `view`th game update: how many of
    the fixture's events come up to and with it (`fixtureTransport` in ui/src/ipc.ts releases
    the first `at` events)."""
    events = [m for m in messages if m.get("kind") == "event"]
    seen = 0
    for index, event in enumerate(events):
        if event.get("event") == "game-update":
            seen += 1
            if seen == view:
                return index + 1
    return len(events)


def shoot(
    messages: list[dict[str, object]],
    view: int,
    layout: str,
    size: str,
    scale: str,
    out: Path,
) -> Path | None:
    """The web interface's screenshot of `messages` at `view`, in a window of `size` points at
    `scale`, written under `out`; None after printing why there is none."""
    width, height = (int(n) for n in size.split("x"))
    name = f"b3d-compare-{os.getpid()}"
    fixture = shots.RECORDED / f"{name}.json"
    fixture.parent.mkdir(parents=True, exist_ok=True)
    fixture.write_text(json.dumps({"messages": messages}))
    phone = "phone&" if layout == "phone" else ""
    shot = {
        "name": "web",
        "route": f"/?{phone}fixture={name}&at={web_at(messages, view)}#/game",
        "width": width,
        "height": height,
        "scale": float(scale),
    }
    listing = out / "shots.json"
    listing.write_text(json.dumps({"shots": [shot]}))
    picture = out / "web.png"
    try:
        env = shots.browser_env()
        with shots.served_ui(env) as url:
            done = subprocess.run(
                ["node", "scripts/shots.mjs", url, str(out), str(listing)],
                cwd=repo_paths.UI,
                env=env,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                check=False,
            )
    except shots.UiError as error:
        print(f"compare: {error}", file=sys.stderr)
        return None
    finally:
        fixture.unlink(missing_ok=True)
    if not picture.exists():
        sys.stderr.write(done.stdout + done.stderr)
        return None
    # The shot's own checks may find faults in the web page; they are printed, not fatal, since
    # the picture is still the web interface's.
    faults = [line for line in done.stdout.splitlines() if line.startswith("ui-shots:")]
    for line in faults:
        print(f"web page: {line}")
    return picture


def run(args: argparse.Namespace) -> int:
    from . import cli

    name, seat, size = target(args)
    common = ["--seat", seat, "--size", size, "--layout", args.layout]
    if args.web:
        return draw(cli, name, common, args, args.web)
    tree = b3d(["tree", name, *common, "--at", args.at])
    if tree is None:
        return USAGE
    view = VIEW_LINE.search(tree)
    if not view:
        print(f"compare: `b3d tree` printed no view line:\n{tree}", file=sys.stderr)
        return 1
    work = Path(tempfile.mkdtemp(prefix="b3d-compare-"))
    try:
        if b3d(["export", str(work), "--only", name]) is None:
            return 1
        exported = work / f"{name.replace('/', '__')}.{seat}.json"
        messages = json.loads(exported.read_text())
        web = shoot(messages, int(view[1]), args.layout, size, args.scale, work)
        if web is None:
            return 1
        # The screenshot is kept beside the picture, since `work` is deleted.
        kept = (
            paths_out() / f"{UNSAFE.sub('_', name)}-{seat}-{size}-web-{os.getpid()}.png"
        )
        shutil.copyfile(web, kept)
        return draw(cli, name, common, args, str(kept))
    finally:
        shutil.rmtree(work, ignore_errors=True)


def draw(
    cli: ModuleType, name: str, common: list[str], args: argparse.Namespace, web: str
) -> int:
    """Runs `b3d-render compare`, which draws the moment beside the screenshot `web`."""
    arguments = [name, *common, "--scale", args.scale, "--at", args.at, "--web", web]
    return cli.run_render("compare")(argparse.Namespace(arguments=arguments))


def paths_out() -> Path:
    """Where pictures go, `$BERYL_CACHE/b3d/out`, as `b3d-render` writes them."""
    out = repo_paths.cache_root() / "b3d" / "out"
    out.mkdir(parents=True, exist_ok=True)
    return out


COMMAND = Command(
    "compare",
    "puts the web interface's screenshot of a moment beside Beryl 3D's frame of it, in one PNG",
    (
        (
            "just b3d compare opening-play --at prompt:1",
            "the play-or-draw prompt in both interfaces",
        ),
        (
            "just b3d compare b3d:pay-default/opponent/1440x900 --at view:2",
            "a test's recording, seat and window",
        ),
    ),
    run,
    arguments,
)
