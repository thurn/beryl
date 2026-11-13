"""`card new <name>`: starts a card's file (docs/project-50k.md, Authoring tools).

It writes `rust/cards/<crate>/<set>/<card>.rs`, where `card path` says the file goes, with the
generated header (card_header.py) and the smallest definition that compiles: each face its
`printed::` constant with no abilities, and an empty `scenarios` module, which every card file
has because its set crate's build script lists each file's `SCENARIOS`. It then names the
implemented card of the same card type whose rules text is nearest (nearest.py), to read as a
model, and the next steps, `just card brief` and `just card check`.

It refuses a card that already has a file, a card complete from the store, whose set crate's
build writes its definition, and a name the store lacks, with `card brief`'s message. An
unimplemented card is a record in the store (plan K2), so this is only for starting work on a
card someone is about to write.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path
from typing import cast

from . import (
    card,
    card_files,
    card_header,
    card_store,
    nearest,
    set_groups,
    store_cards,
)

Json = card_store.Json


def definition(record: Json) -> str:
    """The source of the card's smallest definition, after its header."""
    faces = cast(list[Json], record["faces"])
    printed = ", ".join(
        f"printed::{set_groups.constant(str(f['name']))}" for f in faces
    )
    return (
        "\n"
        "use crate::{names, printed};\n"
        "use beryl_card::{CardDef, card};\n"
        "\n"
        "pub static CARD: CardDef = card(\n"
        f"    names::{set_groups.constant(str(record['name']))},\n"
        f"    &[{printed}],\n"
        ");\n"
        "\n"
        '#[cfg(feature = "scenarios")]\n'
        "pub mod scenarios {\n"
        "    use beryl_scenario_types::Scenario;\n"
        "\n"
        "    pub const SCENARIOS: &[Scenario] = &[];\n"
        "}\n"
    )


def card_types(record: Json) -> str:
    """The card types of the card's first face: "Legendary Planeswalker" of "Legendary
    Planeswalker — Jace"."""
    face = cast(list[Json], record["faces"])[0]
    return str(face.get("type_line", "")).split(" — ")[0]


def model(record: Json, sources: card.Sources) -> tuple[str, Path] | None:
    """The implemented card of the same card types whose rules text is nearest the card's,
    with its file; None when no such card shares a word with it."""
    files = card_files.implemented(sources.cards, sources.store)
    wanted = card_types(record)
    with card_store.Store(sources.store) as store:
        alike = [n for n in files if (r := store.card(n)) and card_types(r) == wanted]
        near = nearest.nearest(
            nearest.rules_text(record), str(record["name"]), store, alike, most=1
        )
    return (near[0], files[near[0]]) if near else None


def run(args: argparse.Namespace, sources: card.Sources = card.Sources()) -> int:
    with card_store.Store(sources.store) as store:
        found = card.find(args.name, store)
    if isinstance(found, str):
        print(f"card: {found}", file=sys.stderr)
        return 1
    name = str(found["name"])
    path = set_groups.SetGroups().card_path(found, sources.cards)
    if path.is_file():
        print(
            f"card new: {name} already has a file, {card.where(path)}", file=sys.stderr
        )
        return 1
    if store_cards.complete(found) is not None:
        print(
            f"card new: {name} is complete from the store: the build of "
            f"{set_groups.SetGroups().group_of(found)} writes its definition, so it needs "
            "no file",
            file=sys.stderr,
        )
        return 1
    crate = path.parent.parent
    if not (crate / "Cargo.toml").is_file():
        print(
            f"card new: there is no set crate {card.where(crate)}; `just card crates` "
            "writes it",
            file=sys.stderr,
        )
        return 1
    path.parent.mkdir(exist_ok=True)
    path.write_text(card_header.header(found) + definition(found))
    # A long name can take a line past rustfmt's width; formatting is best effort.
    subprocess.run(
        ["rustfmt", "--edition", "2024", str(path)], check=False, capture_output=True
    )
    print(f"card new: wrote {card.where(path)}")
    near = model(found, sources)
    if near is not None:
        print(
            f"  The nearest {card_types(found)} implemented: {near[0]}, {card.where(near[1])}"
        )
    print(f'  Next: `just card brief "{name}"`, then `just card check "{name}"`.')
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = commands.add_parser(
        "new", help="start a card's file: its header and the smallest definition"
    )
    p.add_argument("name", help="the card's name or slug, e.g. 'Lightning Bolt'")
    p.set_defaults(func=run)
