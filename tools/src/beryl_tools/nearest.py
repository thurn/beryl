"""Rules texts compared by the words they share: the implemented cards nearest a card, for
`specs` and `card brief`, and what a rules text has in common with any other text.

The implemented cards are the files under `rust/cards` (card_files.py).
"""

from __future__ import annotations

import re
from collections.abc import Iterable
from pathlib import Path
from typing import cast

from . import card_store


def slug(name: str) -> str:
    """A card name in a URL, as `beryl_cli`'s `specs::slug` makes it: `currency-converter`."""
    out = ""
    for c in name:
        if c.isascii() and c.isalnum():
            out += c.lower()
        elif c in " -" and out and not out.endswith("-"):
            out += "-"
    return out.rstrip("-")


WORD: re.Pattern[str] = re.compile(r"[a-z+\-/{}0-9]+")
COMMON: frozenset[str] = frozenset(
    "a an the of to and or it its this that you your each target card cards from".split()
)


def stem(word: str) -> str:
    """A word without its plural or third-person "s": "draws" and "draw" alike."""
    return (
        word[:-1] if len(word) > 3 and word.endswith("s") and word[-2] != "s" else word
    )


def words(text: str, name: str = "", stemmed: bool = False) -> set[str]:
    """A rules text's words and word pairs, less its card's name and the commonest words:
    what two cards' mechanics share. `stemmed` drops each word's "s" (`stem`)."""
    text = text.lower()
    if name:
        text = text.replace(name.lower(), "~")
    found = [w for w in WORD.findall(text) if w not in COMMON]
    if stemmed:
        found = [stem(w) for w in found]
    return set(found) | {f"{a} {b}" for a, b in zip(found, found[1:])}


def overlap(mine: set[str], theirs: set[str]) -> float:
    """How much two texts' words have in common: the share of all their words both have."""
    return len(mine & theirs) / max(1, len(mine | theirs))


def rules_text(record: card_store.Json) -> str:
    """The rules text of each face of a card's record."""
    return "\n".join(
        str(f.get("oracle_text", ""))
        for f in cast(list[card_store.Json], record["faces"])
    )


def nearest(
    text: str,
    name: str,
    store: card_store.Store,
    among: Iterable[str],
    most: int = 5,
) -> list[str]:
    """The names of `among` (never the whole pool) whose cards' rules texts share most with
    `text`, card `name`'s, nearest first; `name` itself and cards sharing nothing are left out.
    """
    mine = words(text, name)
    others = [n for n in among if slug(n) != slug(name)]
    scored: list[tuple[float, str]] = []
    for other, card in zip(others, store.cards(others)):
        shared = overlap(mine, words(rules_text(card), str(card["name"])))
        if shared > 0:
            scored.append((shared, other))
    return [n for _, n in sorted(scored, reverse=True)[:most]]
