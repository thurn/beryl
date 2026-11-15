"""The design tokens as data: `data/design-tokens.json` holds the colors, sizes, fonts and other
style values that both clients draw with, and `ui/src/design/tokens.css` is generated from it.
`by design-tokens` writes the stylesheet; the ui lane fails when it is stale. Beryl 3D's
`beryl_scene` build script reads the same file for its flat layer's constants.

The file has four members:

- `tokens`: every CSS custom property of the web interface's `:root`, in order. Each has a
  `name` (the property without its `--`), a `kind`, a `value` (the CSS text) and an optional
  `note`, written as a comment above it. `group: true` puts a blank line before it. The kind says who can read the value: `color` (a hex
  color, `rgb(r g b / a%)` or `rgb(var(--other))`), `length` (pixels), `number`, `font` (a
  font stack) or `css`, for values only CSS can use (gradients, shadows, images), which the
  Rust side skips.
- `root`: the other declarations of the `:root` rule, after the tokens.
- `rules`: the stylesheet's other rules, each with its `selectors` and `declarations`.
- `flat`: what only Beryl 3D's flat layer uses, which CSS has no property for. Only the build
  script in `rust/beryl3d/crates/beryl_scene/build.rs` reads it.
"""

from __future__ import annotations

import argparse
import json
import re
import textwrap
from pathlib import Path
from typing import Any

from . import paths

DATA: Path = paths.REPO / "data" / "design-tokens.json"
STYLESHEET: Path = paths.UI / "src" / "design" / "tokens.css"
KINDS = ("color", "length", "number", "font", "css")
NAME: re.Pattern[str] = re.compile(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$")
HEADER = (
    "/* Written by `just design-tokens` from data/design-tokens.json: edit that file and run it,\n"
    "   never this one. */\n"
)
# Comments wrap at this many columns, as the hand-written stylesheet did.
WIDTH = 96
# Declarations break past this many columns, as prettier breaks them.
LINE = 80
# A CSS function call: its name and its arguments.
CALL: re.Pattern[str] = re.compile(r"^([a-z-]+)\((.*)\)$", re.S)


def load(path: Path = DATA) -> dict[str, Any]:
    """The tokens file, checked to have the members `render` reads."""
    data: dict[str, Any] = json.loads(path.read_text())
    seen: set[str] = set()
    for token in data["tokens"]:
        name = token["name"]
        if not NAME.match(name):
            raise ValueError(
                f"{path.name}: token name {name!r} is not lower-case-dashed"
            )
        if name in seen:
            raise ValueError(f"{path.name}: token {name!r} appears twice")
        seen.add(name)
        if token["kind"] not in KINDS:
            raise ValueError(
                f"{path.name}: token {name!r} has kind {token['kind']!r}, not one of {KINDS}"
            )
    return data


def comment(note: str, indent: str) -> list[str]:
    """`note` as a CSS comment wrapped at `WIDTH` columns, continuation lines indented to
    line up after the opening `/*`."""
    lines = textwrap.wrap(
        f"/* {note} */",
        WIDTH,
        initial_indent=indent,
        subsequent_indent=indent + "   ",
        break_long_words=False,
        break_on_hyphens=False,
    )
    return lines


def split(text: str) -> list[str]:
    """`text` split at its commas outside parentheses and quotes, each part stripped."""
    parts: list[str] = []
    depth = 0
    quote = ""
    start = 0
    for index, char in enumerate(text):
        if quote:
            quote = "" if char == quote else quote
        elif char in "\"'":
            quote = char
        elif char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        elif char == "," and depth == 0:
            parts.append(text[start:index].strip())
            start = index + 1
    parts.append(text[start:].strip())
    return parts


def layout(text: str, indent: str, head: str, tail: str) -> list[str]:
    """`head + text + tail` at `indent`, broken over lines as prettier breaks a declaration
    longer than `LINE` columns. A list of comma-separated parts goes on the lines after
    `head`, one level deeper, filling each line, or one part per line when a part must break
    itself. A single function call too long for one line puts one argument per line."""
    line = f"{indent}{head}{text}{tail}"
    if len(line) <= LINE:
        return [line]
    deeper = indent + "  "
    parts = split(text)
    if len(parts) > 1:
        out = [f"{indent}{head.rstrip()}"]
        pieces = [
            p + ("," if i < len(parts) - 1 else tail) for i, p in enumerate(parts)
        ]
        if any(len(deeper + piece) > LINE for piece in pieces):
            for i, part in enumerate(parts):
                out.extend(layout(part, deeper, "", pieces[i][len(part) :]))
            return out
        current = ""
        for piece in pieces:
            if current and len(f"{deeper}{current} {piece}") > LINE:
                out.append(deeper + current)
                current = piece
            else:
                current = f"{current} {piece}" if current else piece
        out.append(deeper + current)
        return out
    call = CALL.match(text)
    # A `url(...)` holds one string, which nothing breaks.
    if call and call.group(1) != "url":
        arguments = split(call.group(2))
        out = [f"{indent}{head}{call.group(1)}("]
        for i, argument in enumerate(arguments):
            comma = "," if i < len(arguments) - 1 else ""
            out.extend(layout(argument, deeper, "", comma))
        out.append(f"{indent}){tail}")
        return out
    return [line]


def render(data: dict[str, Any]) -> str:
    """The stylesheet the tokens file describes."""
    out: list[str] = [HEADER.rstrip("\n"), ":root {"]
    for index, token in enumerate(data["tokens"]):
        if token.get("group") and index > 0:
            out.append("")
        note = token.get("note")
        if note:
            out.extend(comment(note, "  "))
        out.extend(layout(token["value"], "  ", f"--{token['name']}: ", ";"))
    if data.get("root"):
        out.append("")
        out.extend(f"  {declaration};" for declaration in data["root"])
    out.append("}")
    for rule in data.get("rules", []):
        out.append("")
        out.append(",\n".join(rule["selectors"]) + " {")
        out.extend(f"  {declaration};" for declaration in rule["declarations"])
        out.append("}")
    return "\n".join(out) + "\n"


def stale(data_path: Path = DATA, stylesheet: Path = STYLESHEET) -> bool:
    """Whether the stylesheet differs from what the tokens file renders."""
    have = stylesheet.read_text() if stylesheet.exists() else ""
    return have != render(load(data_path))


def step() -> int:
    """The ui lane's check: the stylesheet matches the tokens file."""
    try:
        if not stale():
            return 0
    except (ValueError, KeyError) as e:
        print(e)
        return 1
    sheet, data = STYLESHEET.relative_to(paths.REPO), DATA.relative_to(paths.REPO)
    print(f"{sheet} is stale: edit {data}, run `just design-tokens` and commit both")
    return 1


def run(args: argparse.Namespace) -> int:
    if args.check:
        return step()
    STYLESHEET.write_text(render(load()))
    print(f"wrote {STYLESHEET.relative_to(paths.REPO)}")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "design-tokens",
        help="write ui/src/design/tokens.css from data/design-tokens.json (--check)",
    )
    p.add_argument(
        "--check", action="store_true", help="fail when the stylesheet is stale"
    )
    p.set_defaults(func=run)
