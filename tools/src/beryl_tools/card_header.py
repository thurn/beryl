"""A card file's generated header (docs/project-50k.md K2, "A card file"): its card's faces as
printed, from the card store, between two marker lines at the top of the file.

Each face gives its name and mana cost, its type line with its power and toughness, loyalty or
defense, and its Oracle text, wrapped to 100 columns. What a person writes about the card goes
below the end marker, which `just card header` never touches: `just card header <name>`
rewrites one card's header, `--all` every card's. The `names` lane of the checks fails on a
stale header (`card-headers`): in the fast check the changed card files', in slow CI every
file's.
"""

from __future__ import annotations

import argparse
import sys
import textwrap
from collections.abc import Iterable
from pathlib import Path
from typing import cast

from . import card_files, card_store, paths

START = "// Generated from the card store by `just card header`: notes go below the end marker."
END = "// End of the generated card header."
DOC = "//! "
WIDTH = 100


def face_lines(face: card_store.Json) -> list[str]:
    """One face's lines, before the doc comment's prefix."""
    cost = str(face.get("mana_cost") or "")
    lines = [f"{face['name']} {cost}".rstrip()]
    type_line = str(face.get("type_line", ""))
    if "power" in face and "toughness" in face:
        type_line += f", {face['power']}/{face['toughness']}"
    for value in ("loyalty", "defense"):
        if value in face:
            type_line += f", {value} {face[value]}"
    lines.append(type_line)
    text = str(face.get("oracle_text") or "")
    if text:
        lines.append("")
        lines += text.split("\n")
    return lines


def header(record: card_store.Json) -> str:
    """The header of the card `record`, marker lines included, each line ending in a
    newline."""
    out = [START]
    for i, face in enumerate(cast(list[card_store.Json], record["faces"])):
        if i:
            out.append("//!")
        for line in face_lines(face):
            wrapped = textwrap.wrap(
                line,
                WIDTH,
                initial_indent=DOC,
                subsequent_indent=DOC,
                break_long_words=False,
                break_on_hyphens=False,
            )
            out += wrapped or ["//!"]
    out.append(END)
    return "".join(f"{line}\n" for line in out)


def with_header(text: str, record: card_store.Json) -> str:
    """`text`, a card file, with its header made current: the lines between the markers, or
    where there are none yet, the doc comment it starts with."""
    lines = text.splitlines(keepends=True)
    if START + "\n" in lines and END + "\n" in lines:
        start, end = lines.index(START + "\n"), lines.index(END + "\n")
        return "".join(lines[:start]) + header(record) + "".join(lines[end + 1 :])
    first = 0
    while first < len(lines) and lines[first].startswith("//!"):
        first += 1
    rest = "".join(lines[first:])
    return header(record) + (rest if rest.startswith("\n") else "\n" + rest)


def records(
    files: Iterable[Path], store: card_store.Store
) -> list[tuple[Path, card_store.Json]]:
    """Each of `files` that is a card's file, with its card's record."""
    found = []
    for path in files:
        name = card_files.name_of(path, store)
        record = store.card(name) if name is not None else None
        if record is not None:
            found.append((path, record))
    return found


def stale(files: Iterable[Path], store: card_store.Store) -> list[Path]:
    """The card files among `files` whose header is not the store's."""
    return [
        path
        for path, record in records(files, store)
        if with_header(path.read_text(), record) != path.read_text()
    ]


def step(*changed: str) -> int:
    """The `card-headers` check: the card files among `changed` (paths from the repository's
    root), or with none given every card file, each with a current header."""
    files = (
        [paths.REPO / p for p in changed if card_files.FILE.match(p)]
        if changed
        else card_files.files()
    )
    with card_store.Store() as store:
        found = stale([p for p in files if p.is_file()], store)
    for path in found:
        rel = path.relative_to(paths.REPO).as_posix()
        print(f"card-headers: {rel}: the header is stale; `just card header --all`")
    return 1 if found else 0


def run(args: argparse.Namespace) -> int:
    """`card header <names>` or `card header --all`: rewrites the headers."""
    implemented = card_files.implemented()
    if args.all:
        files = list(implemented.values())
    else:
        missing = [n for n in args.names if n not in implemented]
        if missing or not args.names:
            print(
                f"card header: no card file for {missing or 'no card'}", file=sys.stderr
            )
            return 1
        files = [implemented[n] for n in args.names]
    with card_store.Store() as store:
        for path, record in records(files, store):
            path.write_text(with_header(path.read_text(), record))
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    h = commands.add_parser(
        "header", help="rewrite card files' generated headers from the card store"
    )
    h.add_argument("names", nargs="*", help="the cards, e.g. 'Counterspell'")
    h.add_argument("--all", action="store_true", help="every card with a file")
    h.set_defaults(func=run)


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
