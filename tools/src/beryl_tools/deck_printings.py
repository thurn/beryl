"""data/deck-printings.csv: the printings a deck's cards show (`deck,name,set,collector_number,
copies`), which `by cards` writes into the card store as `printing:<set>/<collector_number>`
records."""

from __future__ import annotations

import csv
from collections import Counter
from pathlib import Path
from typing import NamedTuple

from . import paths

DECK_PRINTINGS: Path = paths.REPO / "data" / "deck-printings.csv"


class DeckPrinting(NamedTuple):
    """A row of data/deck-printings.csv: `copies` of the deck's `name` show this printing."""

    deck: str
    name: str
    set: str
    collector_number: str
    copies: int

    @property
    def where(self) -> tuple[str, str]:
        return (self.set, self.collector_number)


def deck_printings(path: Path = DECK_PRINTINGS) -> list[DeckPrinting]:
    """The rows of `path`, in order; a LookupError names a printing listed twice."""
    if not path.exists():
        return []
    with path.open(newline="") as f:
        rows = [
            DeckPrinting(
                r["deck"], r["name"], r["set"], r["collector_number"], int(r["copies"])
            )
            for r in csv.DictReader(f)
        ]
    seen = Counter(r.where for r in rows)
    twice = [where for where, n in seen.items() if n > 1]
    if twice:
        raise LookupError(f"{path.name} lists {twice} twice")
    return rows
