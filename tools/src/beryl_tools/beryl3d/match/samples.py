"""`just b3d match samples [name-prefix]`: the conformance suite for the CSS effects the web
interface draws (docs/beryl-3d.md, "Matching the web interface's look").

Each sample in `data/beryl3d/match/samples.json` is one element with a CSS style, centred on a
page color. The suite shoots each one in WebKit at scale 2 (samples.mjs) and draws it with Beryl
3D's real renderer (`b3d-render sample`), which lays it out as a flat-layer element with the
`Style` its CSS stands for. It prints each sample's largest and mean difference of any color
channel, and `p995`, the difference that 99.5% of pixels are within, with a picture of the web's
pixels, Beryl 3D's and their difference amplified, side by side. A sample passes when `p995` is
at most `TOLERANCE`.

A sample whose CSS `Style` cannot say yet (`unsupported`) is not drawn; it is reported as
unsupported, with the properties it lacks. A sample's text is set in Open Sans, the web
interface's sans-serif face, from the web's own font file on the web side and Beryl 3D's bundled
copy on the other, and its glyph pixels are left out of the score (decision M2): `b3d-render
sample` masks every pixel Beryl 3D's glyphs cover, and the pixels next to them.

Results go under `samples` in `data/beryl3d/web-match.json`, each sample's `p995` or
`"unsupported"`, held by the ratchet rule of `timeline-parity.json`: a value may only go down,
and a supported sample never becomes unsupported. `--update` lowers entries, adds new samples
and, when every sample ran, drops entries of samples that are gone; `--raise` beside it also
raises one, which its commit's body explains.

The web's pictures are kept in `$BERYL_CACHE/b3d/match/samples/web/`, named by a digest of the
page, so a run shoots only the samples whose page changed.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from typing import TypeVar
from pathlib import Path

from ... import paths as repo_paths
from ... import shots
from .. import paths as beryl3d_paths
from . import css

HERE: Path = Path(__file__).resolve().parent
SAMPLES: Path = repo_paths.REPO / "data" / "beryl3d" / "match" / "samples.json"
RATCHET: Path = repo_paths.REPO / "data" / "beryl3d" / "web-match.json"
RENDER_BUILD = ["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"]
# The scale both sides draw at: the desktop app's on a Retina Mac.
SCALE = 2
# The room around a sample's box, in points, where its shadows and glows fall.
MARGIN = 48
# A sample passes when 99.5% of its pixels are within this many levels in every channel.
TOLERANCE = 4
UNSUPPORTED = "unsupported"
T = TypeVar("T")
# The CSS properties Beryl 3D's `Style` can say, as `background`, `border`, `radius`,
# `opacity` and `box_shadow`. `background-image`, `mask-image` and `filter` are drawn when
# `gradients`, `mask` and `filters` can read them.
SIDES: tuple[str, ...] = ("top", "right", "bottom", "left")
SUPPORTED: frozenset[str] = frozenset(
    {"background-color", "border", "border-radius", "opacity", "box-shadow"}
    | {"border-color", "border-width"}
    | {f"border-{side}{part}" for side in SIDES for part in ("", "-color", "-width")}
)
# The filter functions `Style::filter` holds, besides the drop shadows of `Style::drop_shadow`.
FILTERS: frozenset[str] = frozenset({"brightness", "grayscale", "saturate", "blur"})
STOP_COLOR: re.Pattern[str] = re.compile(
    r"rgba?\([^()]*\)|#[0-9a-fA-F]{3,6}\b|transparent"
)
EXTENTS: frozenset[str] = frozenset(
    {"closest-side", "closest-corner", "farthest-side", "farthest-corner"}
)
# The web interface's sans-serif face, which a sample's text is set in, and the web's file of it
# (`ui/src/design/fonts.ts` loads the same one), which the page embeds. Its name is quoted with
# single quotes, since it goes inside the element's double-quoted `style` attribute.
SANS = "'Open Sans Variable'"
SANS_FILE: Path = (
    repo_paths.UI
    / "node_modules"
    / "@fontsource-variable"
    / "open-sans"
    / "files"
    / "open-sans-latin-wght-normal.woff2"
)
# The line height a sample's text is set at on both sides: Beryl 3D's body text's.
LINE_HEIGHT = 1.4

HEX: re.Pattern[str] = re.compile(r"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$")
RGB: re.Pattern[str] = re.compile(
    r"^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)\s*(?:[/,]\s*([\d.]+)(%?))?\s*\)$"
)
BORDER: re.Pattern[str] = re.compile(r"^([\d.]+)px\s+solid\s+(.+)$")
PIXELS: re.Pattern[str] = re.compile(r"^(-?[\d.]+)px$")
SHADOW_COLOR: re.Pattern[str] = re.compile(
    r"rgba?\([^()]*\)|#[0-9a-fA-F]{3,6}\b|transparent"
)


def color(text: str) -> list[int]:
    """A CSS color as `[r, g, b, a]`, every channel from 0 to 255, as Beryl 3D's colors are.
    It reads `transparent`, `#rgb`, `#rrggbb` and `rgb()` or `rgba()`, with or without commas
    and with the alpha as a fraction or a percentage."""
    text = text.strip()
    if text == "transparent":
        return [0, 0, 0, 0]
    found = HEX.match(text)
    if found:
        digits = found[1]
        if len(digits) == 3:
            digits = "".join(c * 2 for c in digits)
        return [int(digits[i : i + 2], 16) for i in (0, 2, 4)] + [255]
    found = RGB.match(text)
    if not found:
        raise ValueError(f"not a color: {text!r}")
    alpha = 1.0
    if found[4] is not None:
        alpha = float(found[4]) / (100 if found[5] else 1)
    return [round(float(found[i])) for i in (1, 2, 3)] + [round(alpha * 255)]


def length(text: str) -> float:
    found = PIXELS.match(text.strip())
    if not found:
        raise ValueError(f"not a length in px: {text!r}")
    return float(found[1])


def shadows(text: str) -> list[dict[str, object]]:
    """A `box-shadow` list as `Style`'s `Shadow`s: `{"x", "y", "blur", "spread", "color",
    "inset"}`, lengths in points and the color's channels from 0 to 255."""
    out: list[dict[str, object]] = []
    for part in css.commas(text):
        inset = bool(re.search(r"\binset\b", part))
        part = re.sub(r"\binset\b", " ", part)
        found = SHADOW_COLOR.search(part)
        shade = color(found[0]) if found else [0, 0, 0, 255]
        rest = part if found is None else part[: found.start()] + part[found.end() :]
        numbers = [0.0 if t == "0" else length(t) for t in rest.split()]
        x, y, blur, spread = (numbers + [0.0] * 4)[:4]
        out.append(
            {
                "x": x,
                "y": y,
                "blur": blur,
                "spread": spread,
                "color": shade,
                "inset": inset,
            }
        )
    return out


def position(text: str) -> float | str:
    """A gradient length as `b3d-render sample` reads it: points, or a percentage as text."""
    text = text.strip()
    if text.endswith("%"):
        return text
    return 0.0 if text == "0" else length(text)


def gradient(text: str) -> dict[str, object] | None:
    """A sample's `linear-gradient()` or `radial-gradient()` as `Style`'s `Gradient`, as
    `b3d-render sample` reads it (`{"kind", "angle"}` or `{"kind", "circle", "extent", "at"}`,
    with `"stops"`), or None when it is repeating or any other image."""
    found = re.fullmatch(r"(linear|radial)-gradient\((.*)\)", text.strip(), re.S)
    if not found:
        return None
    kind, parts = found[1], css.commas(found[2])
    shape = None if STOP_COLOR.match(parts[0]) else parts.pop(0)
    stops: list[dict[str, object]] = []
    for part in parts:
        hit = STOP_COLOR.search(part)
        if hit is None:
            return None
        shade = color(hit[0])
        places = (part[: hit.start()] + " " + part[hit.end() :]).split()
        # A stop with two positions is two stops of its color.
        for place in places or [None]:
            stops.append(
                {"color": shade, "at": None if place is None else position(place)}
            )
    out: dict[str, object] = {"kind": kind, "stops": stops}
    if kind == "linear":
        words = " ".join((shape or "to bottom").split())
        out["angle"] = (
            css.SIDES[words] if words in css.SIDES else float(words.removesuffix("deg"))
        )
        return out
    words = (shape or "").split()
    at: list[float | str] = ["50%", "50%"]
    if "at" in words:
        index = words.index("at")
        at = [position(w) for w in words[index + 1 :]]
        words = words[:index]
    circle = "circle" in words
    words = [w for w in words if w not in ("circle", "ellipse")]
    extent: object = "farthest-corner"
    if words and words[0] in EXTENTS:
        extent = words[0]
    elif words:
        sizes = [position(w) for w in words]
        extent = sizes if len(sizes) == 2 else [sizes[0], sizes[0]]
    out.update({"circle": circle, "extent": extent, "at": at})
    return out


def filter_functions(text: str) -> list[dict[str, object]] | None:
    """A `filter` of only the functions `Style::filter` holds, as `{"fn", "value"}` with a blur
    in points, or None when it has any other function, a drop shadow among them."""
    functions = re.findall(r"([a-z-]+)\(((?:[^()]|\([^()]*\))*)\)", text)
    if not functions or any(name not in FILTERS for name, _ in functions):
        return None
    return [
        {
            "fn": name,
            "value": length(argument) if name == "blur" else css.filter_value(argument),
        }
        for name, argument in functions
    ]


def drop_shadows(text: str) -> list[dict[str, object]] | None:
    """A `filter` of only `drop-shadow()` functions as `Style`'s drop shadows, or None when it
    has any other function."""
    functions = re.findall(r"([a-z-]+)\(((?:[^()]|\([^()]*\))*)\)", text)
    if not functions or any(name != "drop-shadow" for name, _ in functions):
        return None
    return [shadows(argument)[0] for _, argument in functions]


def lacking(sample: dict[str, object]) -> list[str]:
    """What of `sample` Beryl 3D cannot draw yet: each CSS property `Style` lacks. A `filter`
    is drawn when it is only drop shadows or only the functions of `FILTERS`, a
    `background-image` when it is gradients, and a `mask-image` when it is a linear gradient.
    """
    style = sample["style"]
    assert isinstance(style, dict)

    def drawn(name: str, value: str) -> bool:
        if name in SUPPORTED:
            return True
        if name == "filter":
            return (
                drop_shadows(value) is not None or filter_functions(value) is not None
            )
        if name == "background-image":
            return all(gradient(layer) is not None for layer in css.commas(value))
        if name == "mask-image":
            found = gradient(value)
            return found is not None and found["kind"] == "linear"
        return False

    return sorted(name for name in style if not drawn(name, str(style[name])))


def native_text(sample: dict[str, object]) -> dict[str, object] | None:
    """A sample's text as `b3d-render sample` reads it: `{"string", "color", "size"}`."""
    text = sample.get("text")
    if not isinstance(text, dict):
        return None
    return {
        "string": str(text["string"]),
        "color": color(str(text["color"])),
        "size": length(str(text["font-size"])),
    }


def four(values: list[T]) -> list[T]:
    """One to four values of a CSS box property, top, right, bottom and left, as four, the way
    CSS repeats them: one for all, two for top and bottom then the sides, three for top, the
    sides and bottom."""
    if not 1 <= len(values) <= 4:
        raise ValueError(f"not one to four sides: {values!r}")
    top = values[0]
    right = values[1] if len(values) > 1 else top
    bottom = values[2] if len(values) > 2 else top
    left = values[3] if len(values) > 3 else right
    return [top, right, bottom, left]


def solid(text: str) -> tuple[list[int], float]:
    """A `border` shorthand of a width, `solid` and a color."""
    found = BORDER.match(text.strip())
    if not found:
        raise ValueError(f"not a solid border: {text!r}")
    return color(found[2]), float(found[1])


def border(style: dict[str, str]) -> dict[str, object] | None:
    """A sample's border as `Style`'s `Border`, `{"color", "width"}` with four sides each, from
    its `border` shorthand and the longhands after it, in the order CSS applies them: the
    shorthand, then `border-color` and `border-width`, then each side's shorthand, color and
    width. None when it has no border property."""
    if not any(name == "border" or name.startswith("border-") for name in style):
        return None
    colors: list[list[int]] = [[0, 0, 0, 255]] * 4
    widths: list[float] = [0.0] * 4
    if "border" in style:
        shade, width = solid(style["border"])
        colors, widths = [shade] * 4, [width] * 4
    if "border-color" in style:
        colors = four([color(c) for c in STOP_COLOR.findall(style["border-color"])])
    if "border-width" in style:
        widths = four([length(w) for w in style["border-width"].split()])
    for at, side in enumerate(SIDES):
        if f"border-{side}" in style:
            colors[at], widths[at] = solid(style[f"border-{side}"])
        if f"border-{side}-color" in style:
            colors[at] = color(style[f"border-{side}-color"])
        if f"border-{side}-width" in style:
            widths[at] = length(style[f"border-{side}-width"])
    if not any(w > 0 for w in widths):
        return None
    return {"color": colors, "width": widths}


def native_style(style: dict[str, str]) -> dict[str, object]:
    """The `Style` fields that stand for `style`, a sample's CSS of only supported properties,
    as `b3d-render sample` reads them."""
    background = style.get("background-color")
    return {
        "background": None if background is None else color(background),
        "border": border(style),
        "radius": length(style["border-radius"]) if "border-radius" in style else 0.0,
        "opacity": float(style.get("opacity", "1")),
        "box_shadow": shadows(style.get("box-shadow", "")),
        "drop_shadow": drop_shadows(style.get("filter", "")) or [],
        "background_image": [
            gradient(layer) for layer in css.commas(style.get("background-image", ""))
        ],
        "mask": gradient(style["mask-image"]) if "mask-image" in style else None,
        "filter": filter_functions(style.get("filter", "")) or [],
    }


def native_icon(sample: dict[str, object]) -> dict[str, object] | None:
    """A sample's icon as `b3d-render sample` reads it: `{"id", "size", "color"}`, the icon of
    `beryl_scene::widgets::icons` and its side in points."""
    icon = sample.get("icon")
    if not isinstance(icon, dict):
        return None
    return {
        "id": int(icon["id"]),
        "size": int(icon["size"]),
        "color": color(str(icon["color"])),
    }


def window(sample: dict[str, object]) -> tuple[int, int]:
    width, height = sample["box"]  # pyre-ignore[23]: a pair from the JSON
    return int(width) + 2 * MARGIN, int(height) + 2 * MARGIN


def html(sample: dict[str, object]) -> str:
    """The page that shows `sample` on the web: its element in the middle of the page color,
    sized with `box-sizing: border-box`, so a border takes room inside the box as Beryl 3D's
    does. A sample's icon is an inline SVG of the same path data Beryl 3D's artwork copies from
    the web (`beryl_assets/src/icons.rs`), filled in its color and centred in the element.
    """
    style = sample["style"]
    assert isinstance(style, dict)
    width, height = sample["box"]  # pyre-ignore[23]: a pair from the JSON
    declarations = [
        "box-sizing: border-box",
        f"width: {width}px",
        f"height: {height}px",
        *(f"{name}: {value}" for name, value in style.items()),
    ]
    inner = ""
    text = sample.get("text")
    if isinstance(text, dict):
        declarations += [
            "display: flex",
            "align-items: center",
            "justify-content: center",
            f"font-family: {SANS}",
            f"line-height: {LINE_HEIGHT}",
            f"font-size: {text['font-size']}",
            f"color: {text['color']}",
        ]
        inner = str(text["string"])
    icon = sample.get("icon")
    if isinstance(icon, dict):
        declarations += [
            "display: flex",
            "align-items: center",
            "justify-content: center",
        ]
        side = int(icon["size"])
        inner = (
            f"<svg xmlns='http://www.w3.org/2000/svg' viewBox='{icon['view']}' "
            f"width='{side}' height='{side}' style='display: block; fill: {icon['color']}'>"
            f"<path d='{icon['path']}'/></svg>"
        )
    body = (
        f"margin: 0; background: {sample['page']}; width: 100vw; height: 100vh; "
        "display: flex; align-items: center; justify-content: center"
    )
    head = ""
    if isinstance(text, dict):
        font = base64.b64encode(SANS_FILE.read_bytes()).decode()
        head = (
            f"<style>@font-face {{ font-family: {SANS}; font-weight: 300 800; "
            f'src: url(data:font/woff2;base64,{font}) format("woff2"); }}</style>'
        )
    return (
        f'<!doctype html><html><head>{head}</head><body style="{body}">'
        f"<div style=\"{'; '.join(declarations)}\">{inner}</div></body></html>"
    )


def web_pictures(samples: list[dict[str, object]]) -> dict[str, Path] | str:
    """Each sample's WebKit picture, by name, shooting those not in the cache; or why it could
    not shoot them."""
    folder = beryl3d_paths.cache() / "match" / "samples" / "web"
    folder.mkdir(parents=True, exist_ok=True)
    pictures: dict[str, Path] = {}
    missing: list[dict[str, object]] = []
    for sample in samples:
        page = html(sample)
        key = f"{shots.BROWSER}\0{SCALE}\0{window(sample)}\0{page}"
        path = folder / f"{hashlib.sha256(key.encode()).hexdigest()[:20]}.png"
        pictures[str(sample["name"])] = path
        if not path.exists():
            width, height = window(sample)
            missing.append(
                {"html": page, "width": width, "height": height, "png": str(path)}
            )
    if not missing:
        return pictures
    try:
        env = shots.browser_env()
    except shots.UiError as error:
        return str(error)
    with tempfile.TemporaryDirectory(prefix="b3d-samples-") as work:
        job = Path(work) / "job.json"
        job.write_text(json.dumps({"scale": SCALE, "shots": missing}))
        done = subprocess.run(
            ["node", str(HERE / "samples.mjs"), str(job)],
            cwd=repo_paths.UI,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )
    if done.returncode != 0:
        return f"samples.mjs failed:\n{done.stdout}{done.stderr}"
    return pictures


def drawn(
    samples: list[dict[str, object]], pictures: dict[str, Path], out: Path
) -> dict[str, dict[str, object]] | str:
    """Draws each of `samples` with `b3d-render sample` and returns its score by name, or why
    it could not."""
    from .. import cli

    if cli.quick_build(RENDER_BUILD) != 0:
        return "b3d-render did not build"
    job = {
        "scale": SCALE,
        "out": str(out),
        "samples": [
            {
                "name": sample["name"],
                "window": list(window(sample)),
                "page": color(str(sample["page"])),
                "box": sample["box"],
                "web": str(pictures[str(sample["name"])]),
                "style": native_style(sample["style"]),  # pyre-ignore[6]: CSS from JSON
                "text": native_text(sample),
                "icon": native_icon(sample),
            }
            for sample in samples
        ],
    }
    job_file = out / "job.json"
    job_file.write_text(json.dumps(job))
    done = subprocess.run(
        [str(cli.linked("b3d-render")), "sample", "--job", str(job_file)],
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        return f"b3d-render sample failed:\n{done.stdout}{done.stderr}"
    scores = [json.loads(line) for line in done.stdout.splitlines() if line.strip()]
    return {str(score["name"]): score for score in scores}


@dataclass(frozen=True)
class Verdict:
    """What the ratchet says of one sample's result against its entry."""

    # "same", "add" (no entry yet), "lower" (better than its entry) or "raise" (worse, or no
    # longer supported), which only `--update --raise` records.
    kind: str
    # The line to print about it, or None when the result equals its entry.
    note: str | None


def judge(value: int | str, entry: object) -> Verdict:
    """The ratchet's verdict on a sample's `value` (its `p995` or "unsupported") given its
    recorded `entry`, None when it has none. A number may only go down, an unsupported sample
    may become a number, and a number may not become unsupported."""
    if entry is None:
        return Verdict("add", "not listed; --update adds it")
    if value == entry:
        return Verdict("same", None)
    if value == UNSUPPORTED:
        return Verdict("raise", f"was {entry}, now unsupported")
    if isinstance(entry, int) and isinstance(value, int) and value > entry:
        return Verdict("raise", f"rose from {entry} to {value}")
    return Verdict("lower", f"fell to {value} from {entry}; --update lowers it")


def load_ratchet() -> dict[str, object]:
    if RATCHET.exists():
        return json.loads(RATCHET.read_text())
    return {"targets": {}, "samples": {}}


def sample_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "prefix", nargs="?", default="", help="only the samples whose names start so"
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="lower entries of web-match.json and add new samples",
    )
    parser.add_argument(
        "--raise",
        dest="allow_raise",
        action="store_true",
        help="with --update, also raise entries (say why in the commit body)",
    )


def run(args: argparse.Namespace) -> int:
    every = json.loads(SAMPLES.read_text())["samples"]
    chosen = [s for s in every if str(s["name"]).startswith(args.prefix)]
    if not chosen:
        names = ", ".join(str(s["name"]) for s in every)
        print(
            f"match samples: no sample starts with {args.prefix!r}; there are {names}"
        )
        return 2
    supported = [s for s in chosen if not lacking(s)]
    # Every sample is shot, so the web's picture of an unsupported one is at hand for whoever
    # builds what it lacks.
    pictures = web_pictures(chosen)
    if isinstance(pictures, str):
        print(f"match samples: {pictures}", file=sys.stderr)
        return 1
    out = beryl3d_paths.cache() / "match" / "samples" / "out"
    out.mkdir(parents=True, exist_ok=True)
    scores = drawn(supported, pictures, out) if supported else {}
    if isinstance(scores, str):
        print(f"match samples: {scores}", file=sys.stderr)
        return 1

    ratchet = load_ratchet()
    entries = ratchet.setdefault("samples", {})
    assert isinstance(entries, dict)
    broken: list[str] = []
    width = max(len(str(s["name"])) for s in chosen)
    for sample in chosen:
        name = str(sample["name"])
        missing = lacking(sample)
        if missing:
            value: int | str = UNSUPPORTED
            line = f"{UNSUPPORTED}: {', '.join(missing)}  web {pictures[name]}"
        else:
            score = scores[name]
            value = int(score["p995"])
            verdict_word = "pass" if value <= TOLERANCE else "FAIL"
            line = (
                f"{verdict_word} p995 {value} max {score['max']} mean {score['mean']}  "
                f"{score['picture']}"
            )
        verdict = judge(value, entries.get(name))
        print(f"{name:<{width}}  {line}")
        if verdict.note:
            print(f"{'':<{width}}  web-match.json: {verdict.note}")
        if verdict.kind == "same":
            continue
        if args.update and (verdict.kind != "raise" or args.allow_raise):
            entries[name] = value
        elif verdict.kind != "lower":
            broken.append(name)
    if args.update and not args.prefix:
        known = {str(s["name"]) for s in every}
        for name in [n for n in entries if n not in known]:
            del entries[name]
    if args.update:
        ratchet["samples"] = dict(sorted(entries.items()))
        RATCHET.write_text(json.dumps(ratchet, indent=1) + "\n")
    passed = sum(
        1
        for s in chosen
        if not lacking(s) and int(scores[str(s["name"])]["p995"]) <= TOLERANCE
    )
    print(
        f"{len(chosen)} samples: {passed} within {TOLERANCE} levels on 99.5% of pixels, "
        f"{len(chosen) - len(supported)} unsupported"
    )
    if broken:
        print(
            "match samples: web-match.json does not hold "
            + ", ".join(broken)
            + "; fix the look, or --update --raise and say why in the commit body",
            file=sys.stderr,
        )
        return 1
    return 0
