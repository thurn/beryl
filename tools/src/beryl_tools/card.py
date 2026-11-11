"""`card`: the tools a card is written with (docs/project-50k.md, Authoring tools).

`card brief <name>` prints the packet one model writes one card from, in this order:

1. Oracle text: each face's name, mana cost, type line, rules text, and power and toughness,
   loyalty or defense, from the card store, which holds every card in the pool, implemented or
   not.
2. Rulings: Scryfall's, from its `rulings` bulk file in $BERYL_CACHE/scryfall, fetched on first
   use and joined by oracle id.
3. Vocabulary: the entries of beryl_card the rules text most likely needs, read from its source
   as text (vocabulary.py), each with its file, line and doc comment.
4. The nearest implemented cards by rules text (nearest.py), each with its file under
   beryl_cards and the vocabulary entries its definition uses.
5. The XMage class, MIT-licensed, as a reference: its path in the oracle checkout and its source.

Never Forge's script, nor anything derived from one (K16): this module does not import forge.py
and reads nothing under Forge's checkout. A part whose source is missing (offline with no
rulings cached, no XMage checkout) says so and the brief goes on. A name the store lacks exits
1, naming the closest names it has.
"""

from __future__ import annotations

import argparse
import difflib
import gzip
import json
import re
import sys
import urllib.error
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card_store, nearest, paths, scryfall, vocabulary, xmage_classes

Json = card_store.Json


@dataclass(frozen=True)
class Sources:
    """Where a brief reads from: the card store, beryl_card's source (the vocabulary) and
    beryl_cards' (the implemented cards). The rulings and XMage's checkout are in the cache.
    """

    store: Path = card_store.STORE
    vocabulary: Path = vocabulary.SOURCES
    cards: Path = nearest.CARD_SOURCES


def find(name: str, store: card_store.Store) -> Json | str:
    """The card named `name`, by its name, a face's, or either's slug or case; else what to
    say: the closest names the store has."""
    record = store.card(name)
    if record is not None:
        return record
    names = store.names("card")
    wanted = nearest.slug(name)
    for other in names:
        if nearest.slug(other) == wanted:
            return cast(Json, store.card(other))
    folded = {n.casefold(): n for n in names}
    close = difflib.get_close_matches(name.casefold(), list(folded), n=5, cutoff=0.6)
    hint = f"; closest: {', '.join(folded[c] for c in close)}" if close else ""
    return f"no card named {name} in the card store{hint}"


def oracle_text(record: Json) -> str:
    """Each face's printed characteristics, as the card shows them."""
    out: list[str] = []
    for face in cast(list[Json], record["faces"]):
        lines = [f"{face['name']} {face.get('mana_cost', '')}".rstrip()]
        lines.append(str(face.get("type_line", "")))
        if face.get("oracle_text"):
            lines.append(str(face["oracle_text"]))
        if "power" in face:
            lines.append(f"{face['power']}/{face.get('toughness', '')}")
        for stat in ("loyalty", "defense"):
            if stat in face:
                lines.append(f"{stat.capitalize()}: {face[stat]}")
        out.append("\n".join(lines))
    return "\n\n".join(out)


def rulings(oracle_id: str) -> list[Json] | str:
    """Scryfall's rulings on the card with `oracle_id`, oldest first; else why there are none
    to show. A stale file serves when a fresh one cannot be fetched."""
    try:
        path = scryfall.bulk_file("rulings")
    except (urllib.error.URLError, OSError, LookupError, ValueError) as e:
        path = scryfall.cache_dir() / "rulings.jsonl.gz"
        if not path.is_file():
            return (
                f"Scryfall's rulings file is not cached and could not be fetched ({e})"
            )
    needle = oracle_id.encode()
    found: list[Json] = []
    with gzip.open(path, "rb") as lines:
        for line in lines:
            # Most lines are other cards': look for the id before parsing.
            if needle in line:
                ruling = cast(Json, json.loads(line))
                if ruling.get("oracle_id") == oracle_id:
                    found.append(ruling)
    return sorted(found, key=lambda r: str(r.get("published_at", "")))


def definition(path: Path) -> str:
    """A card file's definition: its code before its scenarios, comments left out."""
    text = path.read_text().split("pub const SCENARIOS", 1)[0]
    return "\n".join(line.split("//", 1)[0] for line in text.splitlines())


def uses(entry: vocabulary.Entry, code: str) -> bool:
    """Whether `code` names `entry`: `Effect::Counter` as written, a builder or constant of a
    module (`build::counter`) by its own name."""
    owner, item = entry.name.rsplit("::", 1)
    written = item if owner.islower() else f"{owner}::{item}"
    return re.search(rf"(?<![\w:]){re.escape(written)}\b", code) is not None


def section(title: str, body: str) -> str:
    return f"## {title}\n\n{body.strip()}\n"


def brief(record: Json, sources: Sources) -> str:
    """The packet for the card of `record`."""
    name = str(record["name"])
    text = nearest.rules_text(record)
    parts = [f"# {name}\n", section("Oracle text", oracle_text(record))]

    found = rulings(str(record["oracle_id"]))
    if isinstance(found, str):
        body = f"None shown: {found}."
    elif not found:
        body = "Scryfall has none."
    else:
        body = "\n".join(f"- {r['published_at']}: {r['comment']}" for r in found)
    parts.append(section("Rulings", body))

    entries = vocabulary.entries(sources.vocabulary)
    shown = vocabulary.matching(text, name, entries)
    parts.append(
        section(
            "Vocabulary",
            "\n\n".join(
                f"### {e.name} ({where(e.path)}:{e.line})\n\n{e.doc}" for e in shown
            )
            or "No entry shares a word with its rules text.",
        )
    )

    files = nearest.implemented(sources.cards)
    with card_store.Store(sources.store) as store:
        near = nearest.nearest(text, name, store, files)
    listed: list[str] = []
    for card in near:
        code = definition(files[card])
        used = [e.name for e in entries if uses(e, code)]
        listed.append(
            f"- {card}: {where(files[card])}"
            + (f"\n  uses {', '.join(used)}" if used else "")
        )
    parts.append(
        section(
            "Nearest implemented cards",
            "\n".join(listed)
            or "No implemented card shares a word with its rules text.",
        )
    )

    checkout = xmage_classes.checkout()
    if not checkout.is_dir():
        body = f"None shown: no XMage checkout at {checkout} (`just oracle-setup` makes it)."
    elif (source := xmage_classes.card_source(name, checkout)) is None:
        body = "XMage has no class for it."
    else:
        body = f"{source}\n\n```java\n{source.read_text().strip()}\n```"
    parts.append(section("XMage class", body))
    return "\n".join(parts)


def where(path: Path) -> str:
    """A path in the repository relative to its root; another as it is."""
    return (
        str(path.relative_to(paths.REPO))
        if path.is_relative_to(paths.REPO)
        else str(path)
    )


def run_brief(args: argparse.Namespace, sources: Sources = Sources()) -> int:
    with card_store.Store(sources.store) as store:
        found = find(args.name, store)
    if isinstance(found, str):
        print(f"card: {found}", file=sys.stderr)
        return 1
    print(brief(found, sources))
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("card", help="the tools a card is written with: brief")
    commands = p.add_subparsers(dest="card_command", required=True)
    b = commands.add_parser(
        "brief",
        help="the packet a card is written from: Oracle text, rulings, vocabulary, the "
        "nearest implemented cards and the XMage class",
    )
    b.add_argument("name", help="the card's name or slug, e.g. 'Lightning Bolt'")
    b.set_defaults(func=run_brief)
