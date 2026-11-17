"""What `just b3d match` compares: one moment of a recording, from one seat, in one window at
one scale, and the snapshot directory that holds both clients' pictures and element records of
it (docs/beryl-3d.md, "Matching the web interface's look").

Every subcommand takes the same target as `just b3d compare`: `<recording> --at <moment>
[--seat] [--layout desktop|phone] [--size WxH] [--scale]`, or a test's name
`b3d:<recording>/<seat>/<W>x<H>[@<scale>]`, which gives the seat, the window and the scale.
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path

from .. import paths as beryl3d_paths

# A test's name, as `just b3d test` prints it: b3d:<recording>/<seat>/<width>x<height>, with
# `@<scale>` after the window for a test played at a Retina display's scale.
TEST_NAME: re.Pattern[str] = re.compile(
    r"^b3d:(?P<name>.+)/(?P<seat>you|opponent)/(?P<size>\d+x\d+)(?:@(?P<scale>[\d.]+))?$"
)
# The window each layout's pictures use when `--size` is not given (`beryl_inspect`'s options).
DEFAULT_SIZE = {"desktop": "1600x900", "phone": "393x759"}
# The scale the match tools draw at unless told otherwise: the desktop app's on a Retina Mac.
DEFAULT_SCALE = "2"
# A character that may not stand in a directory's name.
UNSAFE: re.Pattern[str] = re.compile(r"[^A-Za-z0-9.@-]+")


@dataclass(frozen=True)
class Target:
    recording: str
    seat: str
    layout: str
    # <width>x<height> in points.
    size: str
    scale: str
    # The moment, as `just b3d tree --at` takes it.
    at: str

    def width_height(self) -> tuple[int, int]:
        width, height = self.size.split("x")
        return int(width), int(height)

    def test_name(self) -> str:
        """The target's window as a test names it, which `web-match.json` lists."""
        scale = "" if self.scale == "1" else f"@{self.scale}"
        return f"b3d:{self.recording}/{self.seat}/{self.size}{scale}"

    def common(self) -> list[str]:
        """The `b3d` and `b3d-render` options that open this recording, seat and window."""
        return [
            *["--seat", self.seat, "--size", self.size, "--layout", self.layout],
            *["--scale", self.scale],
        ]

    def slug(self) -> str:
        """The snapshot directory's name: everything that tells two targets apart."""
        name = f"{self.recording}-{self.seat}-{self.layout}-{self.size}@{self.scale}-{self.at}"
        return UNSAFE.sub("_", name)

    def directory(self) -> Path:
        """`$BERYL_CACHE/b3d/match/<slug>/`, which holds the snapshot's files."""
        path = beryl3d_paths.cache() / "match" / self.slug()
        path.mkdir(parents=True, exist_ok=True)
        return path


def arguments(parser: argparse.ArgumentParser) -> None:
    """The target's arguments, which every `just b3d match` subcommand takes."""
    parser.add_argument(
        "recording", help="a recording `just b3d list` names, or a test's name"
    )
    parser.add_argument("--at", required=True, help="the moment, as `tree` takes it")
    parser.add_argument("--seat", choices=("you", "opponent"), default=None)
    parser.add_argument("--layout", choices=("desktop", "phone"), default=None)
    parser.add_argument("--size", default=None, help="<width>x<height> in points")
    parser.add_argument(
        "--scale", default=None, help=f"pixels a point (default {DEFAULT_SCALE})"
    )


def phone_under() -> float:
    """The points below which a window's shorter side plays the phone layout, as `b3d` reads a
    test's name (`Layout::for_size`)."""
    space = json.loads((beryl3d_paths.DATA / "space.json").read_text())
    return float(space["flat"]["phone_under"])


def target(args: argparse.Namespace) -> Target:
    """The target `args` name. A test's name gives the seat, window and scale, and the layout
    follows its window as in `just b3d test`; options given beside it win."""
    found = TEST_NAME.match(args.recording)
    if found:
        size = args.size or found["size"]
        width, height = (int(n) for n in size.split("x"))
        layout = args.layout or (
            "phone" if min(width, height) < phone_under() else "desktop"
        )
        return Target(
            recording=found["name"],
            seat=args.seat or found["seat"],
            layout=layout,
            size=size,
            scale=args.scale or found["scale"] or "1",
            at=args.at,
        )
    layout = args.layout or "desktop"
    return Target(
        recording=args.recording,
        seat=args.seat or "you",
        layout=layout,
        size=args.size or DEFAULT_SIZE[layout],
        scale=args.scale or DEFAULT_SCALE,
        at=args.at,
    )
