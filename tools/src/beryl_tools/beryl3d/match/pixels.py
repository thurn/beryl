"""`just b3d match pixels`: where the two clients' pictures of one moment differ, pixel by
pixel, traced to the element answerable on each side (docs/beryl-3d.md, "Matching the web
interface's look").

It takes the snapshot (snapshot.py), pairs its elements (joined.py) and runs `b3d-render
pixels` on the snapshot directory, which does the pixel work in Rust: it leaves out glyph
pixels, groups the pixels that differ by more than 8 levels into regions, gives each to the
topmost painting element at its point on each side (Beryl 3D's by its hit test), and prints the
elements worst first with a hint when one side has nothing there. It writes a magnified crop of
the web, Beryl 3D and their difference for the worst ten into `crops/`, and `pixels.json`.

With `--only <locator>`, both pictures show only that Beryl 3D element, with what is inside it,
and its web pair, on black, which separates an element's own error from what lies behind it.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import replace

from . import joined, snapshot
from .target import arguments, only_argument, target


def pixel_arguments(parser: argparse.ArgumentParser) -> None:
    arguments(parser)
    only_argument(parser)
    parser.add_argument("--fresh", action="store_true", help="take the snapshot again")
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )


def run(args: argparse.Namespace) -> int:
    from .. import cli

    t = target(args)
    out = snapshot.ensure(t, fresh=args.fresh, art=args.art)
    if out is None:
        return 1
    joined.ensure(out)
    if cli.quick_build(snapshot.RENDER_BUILD) != 0:
        print("match pixels: b3d-render did not build", file=sys.stderr)
        return 1
    line = [str(cli.linked("b3d-render")), "pixels", t.recording, *t.common()]
    line += ["--at", t.at, "--dir", str(out)]
    if t.only is not None:
        # The snapshot was drawn from the whole window's, whose pairs name the web element.
        web, why = snapshot.web_pair(replace(t, only=None).directory(), t.only)
        if web is None:
            print(f"match pixels: {why}", file=sys.stderr)
            return 1
        line += ["--only", t.only, "--web-only", str(web)]
    done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode == cli.MISSING_PACK and cli.record() == 0:
        done = subprocess.run(line, capture_output=True, text=True, check=False)
    print(done.stdout, end="")
    print(done.stderr, end="", file=sys.stderr)
    if done.returncode == 0:
        (out / "pixels.txt").write_text(done.stdout)
    return done.returncode
