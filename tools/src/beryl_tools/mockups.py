"""The mockup harness: compares what the UI renders with docs/ui_mockups part by part.

- `ui-compare [part-glob...] [--zoom N] [--review]` renders the replicas (`?replica=<mockup>`,
  dev-only, in ui/src/replica/) at each mockup's native size and writes, per part of
  docs/ui_mockups/regions.json: both crops, a side by side, a 50% onion skin, a difference image
  with the masks blacked out and a flip page. It then reports the verdict ledger (verdicts.py)
  over those parts, listing the passed parts whose replica has changed since. `--review` adds a
  reviewer's prompt.md to each part: hand that packet to a fresh reviewer subagent.
- `ui-verdict <part-glob...> --bead B (--pass | --differs) [--residual T] [--conflict T]` records
  a reviewer's verdict on the parts from the last ui-compare run, and lines it accepted.
- `ui-probe <mockup> <x> <y> [<w> <h>]` prints a point's color, a region's mean color and the
  edges along its middle row and column; `--grid` also writes a zoomed crop with a coordinate grid.
- `ui-fonts <part> --text T` renders candidate font families over a mockup text sample.

Headless Playwright WebKit does the rendering and the image work (ui/scripts/mockups.mjs);
outputs go to the cache (`ui-compare/`, `ui-probe/`, `ui-fonts/` under `<checkout>/`), never into
the tree.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from . import paths, shots, verdicts

# Open-licensed families bundled for the font sheet (ui/src/replica/fonts.ts) and macOS system
# families, referenced by name and never bundled.
OPEN_FONTS = [
    "EB Garamond Variable",
    "Cormorant Garamond Variable",
    "Crimson Pro Variable",
    "Libre Baskerville Variable",
    "Source Serif 4 Variable",
    "Cinzel Variable",
    "Inter Variable",
    "Open Sans Variable",
    "Source Sans 3 Variable",
    "Noto Sans Variable",
]
SYSTEM_FONTS = [
    "Georgia",
    "Baskerville",
    "Hoefler Text",
    "Palatino",
    "Times New Roman",
    "Big Caslon",
    "Iowan Old Style",
    "Charter",
    "Optima",
    "Avenir Next",
    "Helvetica Neue",
    "Gill Sans",
    "-apple-system",
]


def regions() -> dict[str, dict[str, object]]:
    return json.loads(shots.REGIONS.read_text())["mockups"]


def part_names() -> list[str]:
    return [
        f"{mockup}/{part}"
        for mockup, m in regions().items()
        for part in m["parts"]  # pyre-ignore[16]: JSON
    ]


def mockup_png(name: str) -> Path:
    """A mockup by name (`scry`), or any PNG by path (a replica crop from ui-compare)."""
    if name.endswith(".png"):
        return Path(name).resolve()
    found = regions().get(name)
    if found is None:
        raise SystemExit(f"no mockup named {name}; one of {', '.join(regions())}")
    return shots.REGIONS.parent / str(found["file"])


def cache_dir(kind: str) -> Path:
    return paths.cache_root() / kind / paths.REPO.name


def node(env: dict[str, str], *args: str) -> int:
    return subprocess.run(
        ["node", "scripts/mockups.mjs", *args],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def ui_compare(args: argparse.Namespace) -> int:
    globs = args.parts or ["*"]
    names = part_names()
    chosen = [n for n in names if any(fnmatch.fnmatchcase(n, g) for g in globs)]
    if not chosen:
        print(f"ui-compare: no part matches {' '.join(globs)}", file=sys.stderr)
        return 2
    out = cache_dir("ui-compare")
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    commit = verdicts.head()
    try:
        env = shots.webkit_env()
        code = shots.cut_art(env)
        if code == 0:
            with shots.served_ui(env) as url:
                code = node(
                    env,
                    "compare",
                    url,
                    str(shots.REGIONS),
                    str(out),
                    str(args.zoom),
                    *chosen,
                )
    except shots.UiError as e:
        print(f"ui-compare: {e}", file=sys.stderr)
        code = 1
    found = out / verdicts.FINGERPRINTS
    if code != 0 or not found.exists():
        # A page error makes the render suspect: no verdict may be recorded against it.
        found.unlink(missing_ok=True)
        return code or 1
    fingerprints = json.loads(found.read_text())
    found.write_text(
        json.dumps({"commit": commit, "parts": fingerprints}, indent=2) + "\n"
    )
    ledger = verdicts.parts(verdicts.load())
    if args.review:
        for path in verdicts.write_packets(out, ledger, chosen, args.zoom):
            print(path)
    print(verdicts.report(ledger, fingerprints))
    return 0


def ui_verdict(args: argparse.Namespace) -> int:
    raw = verdicts.load()
    ledger = verdicts.parts(raw)
    names = [n for n in ledger if any(fnmatch.fnmatchcase(n, g) for g in args.parts)]
    if not names:
        print(f"ui-verdict: no part matches {' '.join(args.parts)}", file=sys.stderr)
        return 2
    verdict = "pass" if args.pass_ else "differs" if args.differs else None
    if verdict is None and not (args.residual or args.conflict):
        print(
            "ui-verdict: give --pass, --differs, --residual or --conflict",
            file=sys.stderr,
        )
        return 2
    commit, fingerprints = verdicts.last_run(cache_dir("ui-compare"))
    missing = [n for n in names if n not in fingerprints]
    if verdict is not None and missing:
        print(
            f"ui-verdict: run `just ui-compare` on {' '.join(missing)} first: a verdict"
            " records the render it judged",
            file=sys.stderr,
        )
        return 2
    verdicts.record(
        raw,
        names,
        verdict=verdict,
        bead=args.bead,
        commit=commit,
        fingerprints=fingerprints,
        residuals=args.residual or [],
        conflicts=args.conflict or [],
    )
    verdicts.save(raw)
    for name in names:
        print(f"{name}: {verdict or 'accepted lines'} ({args.bead} at {commit or '?'})")
    return 0


def ui_probe(args: argparse.Namespace) -> int:
    rect = [args.x, args.y] + ([args.w, args.h] if args.w is not None else [])
    extra = ["--threshold", str(args.threshold)]
    if args.grid:
        size = "x".join(str(v) for v in rect[2:]) or "1x1"
        stem = Path(args.mockup).stem
        grid = cache_dir("ui-probe") / f"{stem}-{args.x}-{args.y}-{size}.png"
        extra += ["--grid", str(grid), "--zoom", str(args.zoom)]
        if args.step:
            extra += ["--step", str(args.step)]
    try:
        env = shots.webkit_env()
    except shots.UiError as e:
        print(f"ui-probe: {e}", file=sys.stderr)
        return 1
    return node(
        env, "probe", str(mockup_png(args.mockup)), *(str(v) for v in rect), *extra
    )


def sample_rect(part: str) -> tuple[str, list[int]]:
    """`mockup/part` from the region file, or `mockup:x,y,w,h`."""
    if ":" in part:
        mockup, spec = part.split(":", 1)
        rect = [int(v) for v in spec.split(",")]
        if len(rect) != 4:
            raise SystemExit("ui-fonts: a sample is mockup:x,y,w,h")
        return mockup, rect
    mockup, _, name = part.partition("/")
    spec = regions().get(mockup, {}).get("parts", {}).get(name)  # pyre-ignore[16]
    if spec is None:
        raise SystemExit(f"ui-fonts: no part {part}")
    return mockup, list(spec if isinstance(spec, list) else spec["rect"])


def ui_fonts(args: argparse.Namespace) -> int:
    mockup, rect = sample_rect(args.part)
    families = args.family or OPEN_FONTS + SYSTEM_FONTS
    spec = {
        "mockup": str(mockup_png(mockup)),
        "rect": rect,
        "text": args.text,
        "size": args.size or round(rect[3] * 0.7),
        "weight": args.weight,
        "italic": args.italic,
        "color": args.color,
        "align": args.align,
        "zoom": args.zoom,
        "families": [
            {"name": f, "bundled": f in OPEN_FONTS, "system": f in SYSTEM_FONTS}
            for f in families
        ],
    }
    slug = re.sub(r"[^A-Za-z0-9]+", "-", args.part).strip("-")
    out = cache_dir("ui-fonts") / f"{slug}.png"
    try:
        env = shots.webkit_env()
        with tempfile.NamedTemporaryFile("w", suffix=".json") as f:
            json.dump(spec, f)
            f.flush()
            with shots.served_ui(env) as url:
                return node(env, "fonts", url, str(out), f.name)
    except shots.UiError as e:
        print(f"ui-fonts: {e}", file=sys.stderr)
        return 1


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("ui-compare", help="compare replicas with docs/ui_mockups")
    p.add_argument(
        "parts", nargs="*", help="globs over mockup/part names (regions.json)"
    )
    p.add_argument("--zoom", type=int, default=1, help="nearest-neighbor zoom")
    p.add_argument(
        "--review", action="store_true", help="also write each part's reviewer prompt"
    )
    p.set_defaults(func=ui_compare)

    p = sub.add_parser(
        "ui-verdict", help="record pixel verdicts in docs/ui_mockups/regions.json"
    )
    p.add_argument("parts", nargs="+", help="globs over mockup/part names")
    p.add_argument("--bead", required=True, help="the bead whose reviewer judged")
    verdict = p.add_mutually_exclusive_group()
    verdict.add_argument(
        "--pass",
        dest="pass_",
        action="store_true",
        help="indistinguishable at a glance",
    )
    verdict.add_argument("--differs", action="store_true", help="distinguishable")
    p.add_argument(
        "--residual", action="append", help="an accepted residual (repeatable)"
    )
    p.add_argument(
        "--conflict", action="append", help="an accepted mockup conflict (repeatable)"
    )
    p.set_defaults(func=ui_verdict)

    p = sub.add_parser("ui-probe", help="colors and edges of a mockup region")
    p.add_argument("mockup", help="a mockup name (scry) or a PNG path")
    p.add_argument("x", type=int)
    p.add_argument("y", type=int)
    p.add_argument("w", type=int, nargs="?")
    p.add_argument("h", type=int, nargs="?")
    p.add_argument(
        "--threshold", type=int, default=24, help="smallest channel step for an edge"
    )
    p.add_argument("--grid", action="store_true", help="write a zoomed, gridded crop")
    p.add_argument("--zoom", type=int, default=8)
    p.add_argument("--step", type=int, help="grid spacing in mockup pixels")
    p.set_defaults(func=ui_probe)

    p = sub.add_parser("ui-fonts", help="font candidates over a mockup text sample")
    p.add_argument("part", help="mockup/part, or mockup:x,y,w,h")
    p.add_argument("--text", required=True, help="the sample's text")
    p.add_argument("--size", type=float, help="font size in mockup pixels")
    p.add_argument("--weight", type=int, default=400)
    p.add_argument("--italic", action="store_true")
    p.add_argument("--color", default="#F2ECE1")
    p.add_argument("--align", choices=["left", "center", "right"], default="center")
    p.add_argument("--zoom", type=int, default=2)
    p.add_argument(
        "--family", action="append", help="a family to try (repeatable); default all"
    )
    p.set_defaults(func=ui_fonts)
