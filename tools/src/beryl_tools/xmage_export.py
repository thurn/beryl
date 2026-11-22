"""Exports scenarios as XMage tests: one test method per scenario, each written by an
`xmage.Exporter`, or marked unsupported with the reason."""

from __future__ import annotations

from collections.abc import Sequence
from typing import cast

from .card_facts import Json, Unsupported
from .xmage import Exporter
from .xmage_java import Export, Faces, method_name, setup_cards, xmage_spelling


def export_one(entry: Json, index: int, faces: Faces, cards: set[str]) -> Export:
    return _export_one(entry, index, faces, {xmage_spelling(c) for c in cards})


def _export_one(entry: Json, index: int, faces: Faces, known: set[str]) -> Export:
    """Translate with the oracle's normalized card names shared across the dump."""
    export = Export(
        card=cast(str, entry["card"]),
        scenario=cast(str, entry["name"]),
        method=method_name(index, cast(str, entry["name"])),
    )
    if "failure" in entry:
        export.unsupported = f"Beryl fails it: {entry['failure']}"
        return export
    try:
        writer = Exporter(entry, faces, export)
        missing = sorted(
            c
            for c in {writer.cards.printed_name(n) for n in setup_cards(entry)}
            if xmage_spelling(c) not in known
        )
        if missing:
            raise Unsupported(f"XMage lacks {', '.join(missing)}")
        writer.run()
    except Unsupported as e:
        export.unsupported = str(e)
        export.body.clear()
    return export


def export_all(dump: Sequence[Json], faces: Faces, cards: set[str]) -> list[Export]:
    known = {xmage_spelling(c) for c in cards}
    counters: dict[str, int] = {}
    exports = []
    for entry in dump:
        card = cast(str, entry["card"])
        counters[card] = counters.get(card, 0) + 1
        exports.append(_export_one(entry, counters[card], faces, known))
    return exports
