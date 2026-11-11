"""XMage's card classes: the class XMage names a card's, and where its source is in the
oracle checkout. The exporter (xmage.py) names its test classes after them, and `card brief`
prints a card's as a reference.
"""

from __future__ import annotations

import re
import unicodedata
from pathlib import Path

from . import paths


def card_class(card: str) -> str:
    """The class XMage names a card's: its name's letters and digits, each word capitalized,
    accents dropped: `JaceTheMindSculptor`, `FireIce`."""
    folded = unicodedata.normalize("NFKD", card.replace("Æ", "Ae").replace("æ", "ae"))
    words = re.sub(r"[^A-Za-z0-9 ]", "", folded).split()
    return "".join(w[0].upper() + w[1:] for w in words)


def checkout() -> Path:
    """XMage's checkout, where `just oracle-setup` clones it: oracles.checkout(oracles.XMAGE),
    which card brief cannot import (oracles imports forge.py: K16)."""
    return paths.cache_root() / "oracles" / "xmage"


def card_source(card: str, root: Path) -> Path | None:
    """The source of a card's class in the XMage checkout at `root`: by its name, else by its
    first face's, else as a set file lists it (`new SetCardInfo("Name", ..., mage.cards.x.Class
    .class)`); None if XMage has no such card."""
    cards = root / "Mage.Sets" / "src" / "mage" / "cards"
    for name in dict.fromkeys((card, card.split(" // ")[0])):
        found = card_class(name)
        if found and (path := cards / found[0].lower() / f"{found}.java").is_file():
            return path
    listed = re.compile(
        r'new SetCardInfo\("' + re.escape(card.split(" // ")[0]) + r'"[^;]*?'
        r"mage\.cards\.(\w)\.(\w+)\.class"
    )
    for sets in sorted((root / "Mage.Sets" / "src" / "mage" / "sets").glob("*.java")):
        if match := listed.search(sets.read_text(errors="replace")):
            return cards / match.group(1) / f"{match.group(2)}.java"
    return None
