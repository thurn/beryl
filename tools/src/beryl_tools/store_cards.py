"""The cards complete from the card store, which need no file.

A pool card is complete from the store when it is a single-faced creature, with a mana cost and
power and toughness the engine can hold, whose Oracle text, with its reminder text removed, is
empty or only keyword abilities the engine has. The engine's
keywords are the constants of `Keywords` in `beryl_card`'s `characteristics.rs`, read from that
source as the build scripts read it (`beryl_card_build`'s `complete.rs`), so no second list of
keywords is kept. A set crate's build script writes each such card's definition; a card with a
file is built from its file instead.
"""

from __future__ import annotations

import functools
import re
from pathlib import Path
from typing import cast

from . import card_store, paths

Json = card_store.Json

# Where `Keywords` is defined, from the repository's root.
KEYWORDS_SOURCE_REL = "rust/crates/beryl_card/src/characteristics.rs"
KEYWORDS_SOURCE: Path = paths.REPO / KEYWORDS_SOURCE_REL
# A constant of the `Keywords` bitset: `NAME = bit,`.
CONSTANT: re.Pattern[str] = re.compile(r"^\s*([A-Z][A-Z0-9_]*) = \d+,\s*$")
# A mana cost `beryl_card::mana` can hold (`mana_notation.rs`): numbers, W, U, B, R, G, C and X.
COST: re.Pattern[str] = re.compile(r"^(?:\{(?:\d+|[WUBRGCX])\})*$")
# A power or toughness the engine can hold: a number, or `*`, which needs rules text anyway.
STRENGTH: re.Pattern[str] = re.compile(r"^-?\d+$")
# Reminder text, in parentheses (CR 207.2a).
REMINDER: re.Pattern[str] = re.compile(r"\([^()]*\)")


@functools.cache
def keywords(source: Path = KEYWORDS_SOURCE) -> dict[str, str]:
    """The engine's keyword abilities, each as Oracle text spells it in lower case, with its
    constant: `FIRST_STRIKE` is "first strike"."""
    text = source.read_text()
    start = text.index("Keywords(u64) {")
    found: dict[str, str] = {}
    for line in text[start:].splitlines()[1:]:
        if line.strip() == "}":
            break
        if match := CONSTANT.match(line):
            found[match[1].lower().replace("_", " ")] = match[1]
    return found


def complete(record: Json, known: dict[str, str] | None = None) -> list[str] | None:
    """The keyword constants of the card of `record` when it is complete from the store; None
    when it needs a file."""
    known = keywords() if known is None else known
    faces = cast(list[Json], record.get("faces", []))
    if len(faces) != 1 or not record.get("first_set") or record["layout"] != "normal":
        return None
    face = faces[0]
    card_types = str(face.get("type_line", "")).split(" — ")[0]
    if "Creature" not in card_types.split(" "):
        return None
    if not COST.match(str(face.get("mana_cost") or "")) or not all(
        STRENGTH.match(str(face.get(s, "0"))) for s in ("power", "toughness")
    ):
        return None
    text = REMINDER.sub("", str(face.get("oracle_text") or ""))
    found: list[str] = []
    for words in re.split(r"[\n,]", text):
        words = words.strip().lower()
        if not words:
            continue
        if words not in known:
            return None
        if known[words] not in found:
            found.append(known[words])
    return found


def names(store: Path = card_store.STORE) -> list[str]:
    """Every card complete from the store, by name, files or not."""
    known = keywords()
    with card_store.Store(store) as opened:
        return sorted(
            str(record["name"])
            for kind, record in opened
            if kind == "card"
            and "alias" not in record
            and complete(record, known) is not None
        )
