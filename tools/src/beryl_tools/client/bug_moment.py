"""`just client bug <report> --png | --film <ms> | --story <name>`: draws a bug report's moment
on the current tree from the report's own capture, and writes it as a story.

Fixes have been checked on states the player never reported, such as a card built by hand in
the stack zone when the player saw a resolved creature still drawn at the stack's position. So
this draws the reported moment itself, in the report's window size, scale and seat, cut to the
box the player selected, beside the report's own `screenshot.png`.

The capture is one of two things. A report saved while a recording replayed names the recording
and the moment in its `ui.json` log. A report saved in a live game holds `fixture.json`, the
latest views the client was given, which the `client` program opens as a recording
(`beryl_inspect::fixture_file`), and `tree.txt`, the element tree of the frozen frame. The report
does not say how long after the last view the player froze the table, so the moment is the frame
whose tree is nearest `tree.txt` (`just client moment`), unless `--at` gives it.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from .. import paths
from . import paths as client_paths

if TYPE_CHECKING:
    from .bug import Opened

# Where `--story <name>` writes, so that `just client test story/game_table/<name>` runs it.
STORIES: Path = Path("client/crates/game_table/stories")
# A match whose distance (`just client moment`) averages more than this many points an element
# is too far from the report's tree to trust without looking.
FAR_PER_ELEMENT: int = 20
STORY_NAME: re.Pattern[str] = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")


@dataclass(frozen=True)
class Window:
    # As `--size` takes it, `<width>x<height>` in logical points.
    size: str
    scale: str
    # The player's selection as `--region` takes it, or none when it is the whole window.
    region: str | None


@dataclass(frozen=True)
class Capture:
    # A recording's name, or the path of the fixture file the views are read from.
    source: str
    seat: str
    # The reported moment as `--at` takes it, when the report says it.
    moment: str | None


def number(value: float) -> str:
    return f"{value:g}"


def window(page: dict[str, Any], region: dict[str, Any] | None) -> Window | None:
    """The report's window, from its `ui.json` `page` and `region`; none for a report from the
    web interface, which has no window size."""
    try:
        width, height = round(float(page["width"])), round(float(page["height"]))
        scale = float(page.get("scale") or 1)
    except (KeyError, TypeError, ValueError):
        return None
    box = None
    if region:
        values = [float(region[key]) for key in ("x", "y", "width", "height")]
        if values != [0.0, 0.0, float(width), float(height)]:
            box = ",".join(number(value) for value in values)
    return Window(f"{width}x{height}", number(scale), box)


def trimmed(fixture: Path, views: int, out: Path) -> Path:
    """A copy of `fixture` at `out` that keeps only its last `views` views and every other
    message, such as the recorded answers."""
    data = json.loads(fixture.read_text(encoding="utf-8"))
    messages: list[dict[str, Any]] = (
        data["messages"] if isinstance(data, dict) else data
    )
    updates = [
        index
        for index, message in enumerate(messages)
        if message.get("event") == "game-update"
    ]
    dropped = set(updates[: max(len(updates) - views, 0)])
    kept = [message for index, message in enumerate(messages) if index not in dropped]
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"messages": kept}), encoding="utf-8")
    return out


def capture(opened: Opened, views: int | None, scratch: Path) -> Capture | str:
    """What the report's moment is drawn from, or why it cannot be."""
    log = opened.log
    if isinstance(log, dict) and "recording" in log and "at" in log:
        moment = "end" if log.get("ended") else f"{log['at']}ms"
        return Capture(str(log["recording"]), str(log.get("seat", "you")), moment)
    fixture = opened.report.folder / "fixture.json"
    if not fixture.is_file():
        return (
            "the report has neither a recording moment nor fixture.json, so its moment "
            "cannot be drawn; follow the reproduce commands"
        )
    if views is not None:
        fixture = trimmed(fixture, views, scratch / f"fixture-last-{views}.json")
    return Capture(str(fixture), "you", None)


def scene_arguments(found: Capture, shape: Window) -> list[str]:
    return [
        found.source,
        *["--seat", found.seat, "--size", shape.size, "--scale", shape.scale],
    ]


def run_program(name: str, arguments: list[str]) -> subprocess.CompletedProcess[str]:
    """Builds the `client` or `client-render` program and runs it with `arguments`."""
    from .cli import launch_program, quick_build

    package = "beryl_inspect" if name == "client" else "beryl_pictures"
    built = quick_build(["build", "-q", "-p", package, "--bin", name])
    if built:
        return subprocess.CompletedProcess[str]([name, *arguments], built, "", "")
    return launch_program(name, arguments, want=1, capture=True)


def json_of(name: str, arguments: list[str]) -> dict[str, Any] | str:
    """The JSON the program prints for `arguments`, or its error."""
    done = run_program(name, [*arguments, "--json"])
    if done.returncode:
        return done.stderr.strip() or f"{name} {arguments[0]} failed"
    return json.loads(done.stdout.strip().splitlines()[-1])


def reported_moment(opened: Opened, found: Capture, shape: Window) -> str:
    """The moment the report shows: the one it names, else the frame nearest its tree, else the
    end of its views."""
    if found.moment is not None:
        return found.moment
    tree = opened.report.folder / "tree.txt"
    if not tree.is_file():
        print(
            "the report has no tree.txt, so its moment is taken as the end of its views"
        )
        return "end"
    nearest = json_of(
        "client", ["moment", *scene_arguments(found, shape), "--like", str(tree)]
    )
    if isinstance(nearest, str):
        print(f"no moment matched the report's tree ({nearest}); using the end")
        return "end"
    print(
        f"moment: {nearest['at_ms']}ms is nearest the report's tree.txt "
        f"(distance {nearest['distance']} over {nearest['elements']} elements; "
        "0 is the same tree)"
    )
    if nearest["distance"] > FAR_PER_ELEMENT * nearest["elements"]:
        print(
            "  no frame is close to the report's tree: the views kept (--views) may start "
            "after the reported moment, or this tree draws the table differently. Compare "
            "the picture with the screenshot, or give --at"
        )
    return f"{nearest['at_ms']}ms"


def film_span(
    found: Capture, shape: Window, moment: str, span_ms: int
) -> tuple[str, str] | str:
    """`--from` and `--to` for `span_ms` either side of `moment`, kept within the views."""
    info = json_of("client", ["info", *scene_arguments(found, shape)])
    if isinstance(info, str):
        return info
    end = int(info["end_ms"])
    at = end if moment == "end" else int(moment.removesuffix("ms"))
    return f"{max(at - span_ms, 0)}ms", f"{min(at + span_ms, end)}ms"


def story_text(text: str, name: str, words: str) -> str:
    """`text`, which `story from` wrote, with `name` as its capture's source instead of the
    path of the report's fixture, which names a folder on the reporting machine, and a
    comment that says what the player reported."""
    lines = text.splitlines(keepends=True)
    out = lines[:1]
    first = (words.strip().splitlines() or ["(no description)"])[0]
    out.append(f"// A bug report's moment. The player said: {first}\n")
    for line in lines[1:]:
        if line.startswith("capture "):
            # The source is a path and may hold spaces; the four words after it do not.
            tail = line.rstrip("\n").rsplit(" ", 4)[1:]
            line = f"capture {name} {' '.join(tail)}\n"
        out.append(line)
    return "".join(out)


def run(opened: Opened, args: argparse.Namespace, words: str) -> int:
    """Draws, films or writes as a story the report's moment, as `args` asks."""
    ui = opened.report.folder / "ui.json"
    data = json.loads(ui.read_text(encoding="utf-8")) if ui.is_file() else {}
    shape = window(data.get("page") or {}, data.get("region"))
    if shape is None:
        print("bug: the report's ui.json gives no window size")
        return 1
    scratch = client_paths.cache() / "bugs" / opened.report.id
    found = capture(opened, args.views, scratch)
    if isinstance(found, str):
        print(f"bug: {found}")
        return 1
    moment = args.at or reported_moment(opened, found, shape)
    actions = [part for action in args.do for part in ("--do", action)]
    scene = [*scene_arguments(found, shape), *actions]
    screenshot = opened.report.folder / "screenshot.png"
    beside = ["--beside", str(screenshot)] if screenshot.is_file() else []
    region = ["--region", shape.region] if shape.region else []
    if args.png:
        done = run_program(
            "client-render", ["png", *scene, "--at", moment, *region, *beside]
        )
        print(done.stdout or done.stderr, end="")
        if done.returncode:
            return done.returncode
    if args.film is not None:
        span = film_span(found, shape, moment, args.film)
        if isinstance(span, str):
            print(f"bug: {span}")
            return 1
        start, end = span
        done = run_program(
            "client-render",
            ["film", *scene, "--from", start, "--to", end, *region, *beside],
        )
        print(done.stdout or done.stderr, end="")
        if done.returncode:
            return done.returncode
    if beside and (args.png or args.film is not None):
        print(
            "the report's screenshot is on the left, this tree's picture on the right"
        )
    if args.story is not None:
        return write_story(args.story, scene, moment, words)
    return 0


def write_story(name: str, scene: list[str], moment: str, words: str) -> int:
    if not STORY_NAME.fullmatch(name):
        print(f"bug: --story {name!r}: use lowercase words joined by hyphens")
        return 2
    path = paths.REPO / STORIES / f"{name}.story"
    if path.exists():
        print(f"bug: {path} already exists")
        return 1
    temporary = client_paths.cache() / "bugs" / f"{name}.story"
    temporary.unlink(missing_ok=True)
    done = run_program(
        "client", ["story", "from", *scene, "--at", moment, "--out", str(temporary)]
    )
    if done.returncode:
        print(done.stderr or done.stdout, end="")
        return done.returncode
    path.write_text(story_text(temporary.read_text(encoding="utf-8"), name, words))
    temporary.unlink()
    size = path.stat().st_size // 1024
    print(f"wrote {path.relative_to(paths.REPO)} ({size} KB)")
    if size > 200:
        print("  it is large: --views <n> keeps only the report's last n views")
    print(f"  run it with: just client test story/game_table/{name}")
    return 0


def arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--png",
        action="store_true",
        help="draw the reported moment on this tree beside the report's screenshot",
    )
    parser.add_argument(
        "--film",
        type=int,
        metavar="MS",
        help="film the MS milliseconds either side of the reported moment",
    )
    parser.add_argument(
        "--do",
        action="append",
        default=[],
        metavar="ACTION",
        help="an input to give at the moment first, as `just client tree --do` takes it",
    )
    parser.add_argument(
        "--at",
        help="the moment to draw, as `just client tree --at` takes it, instead of the one "
        "nearest the report's tree.txt",
    )
    parser.add_argument(
        "--views",
        type=int,
        metavar="N",
        help="keep only the last N views of the report's fixture.json",
    )
    parser.add_argument(
        "--story",
        metavar="NAME",
        help=f"write the moment as {STORIES.as_posix()}/NAME.story, a regression guard",
    )
