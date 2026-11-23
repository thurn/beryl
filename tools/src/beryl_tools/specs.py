"""Card name resolution and nearby implemented cards for native spec inspection."""

from __future__ import annotations

import sys
import urllib.error
import urllib.parse

from . import card_files, card_store, cards, scryfall, store_cards
from . import nearest as nearest_cards

slug = nearest_cards.slug


def implemented() -> set[str]:
    """The slugs of the implemented cards: each card with a file and each complete from the
    card store, which has none (store_cards.py), by its name. A store card's spec is generated
    only when someone asks for it by name."""
    names = [*card_files.implemented(), *store_cards.names()]
    return {slug(name) for name in names}


def oracle(name: str) -> str | None:
    """A card's rules text: the card store's (by its name, or the slug of a deck card's), else
    Scryfall's; None offline or unknown."""
    with card_store.Store() as store:
        deck = {slug(n): n for n in cards.m1_names()}
        record = store.card(deck.get(slug(name), name))
    if record is not None:
        return nearest_cards.rules_text(record)
    url = "https://api.scryfall.com/cards/named?fuzzy=" + urllib.parse.quote(name)
    try:
        found = scryfall.http_get(url)
    except (urllib.error.URLError, OSError, ValueError):
        return None
    faces = found.get("card_faces")
    if isinstance(faces, list):
        return "\n".join(str(f.get("oracle_text", "")) for f in faces)
    return str(found.get("oracle_text", ""))


def nearest(name: str, specced: set[str], most: int = 5) -> list[str]:
    """The specced cards whose rules text shares most with `name`'s, nearest first."""
    text = oracle(name)
    if text is None:
        return []
    among = [n for n in card_files.implemented() if slug(n) in specced]
    with card_store.Store() as store:
        return nearest_cards.nearest(text, name, store, among, most)


def resolve(names: list[str]) -> list[str] | None:
    """The slugs of `names` (slugs or card names); None, having said which have no spec and
    the specced cards nearest each in rules text, if any has none."""
    specced = implemented()
    slugs = [slug(n) for n in names]
    unknown = [n for n, s in zip(names, slugs) if s not in specced]
    for n in unknown:
        near = nearest(n, specced)
        hint = f"; nearest specced: {', '.join(near)}" if near else ""
        print(f"specs: no spec for {n}{hint}", file=sys.stderr)
    return None if unknown else slugs
