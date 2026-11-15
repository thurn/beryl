"""Exports scenarios as XMage tests: one test method per scenario, each written by an
`xmage.Exporter`, or marked unsupported with the reason."""

from __future__ import annotations

from collections.abc import Sequence
from typing import cast

from .card_facts import Json, Unsupported
from .xmage import Exporter
from .xmage_java import Export, Faces, method_name, setup_cards, xmage_spelling


def export_one(entry: Json, index: int, faces: Faces, cards: set[str]) -> Export:
    export = Export(
        card=cast(str, entry["card"]),
        scenario=cast(str, entry["name"]),
        method=method_name(index, cast(str, entry["name"])),
    )
    if "failure" in entry:
        export.unsupported = f"Beryl fails it: {entry['failure']}"
        return export
    known = {xmage_spelling(c) for c in cards}
    missing = sorted(
        c for c in set(setup_cards(entry)) if xmage_spelling(c) not in known
    )
    if missing:
        export.unsupported = f"XMage lacks {', '.join(missing)}"
        return export
    try:
        Exporter(entry, faces, export).run()
    except Unsupported as e:
        export.unsupported = str(e)
        export.body.clear()
    return export


def export_all(dump: Sequence[Json], faces: Faces, cards: set[str]) -> list[Export]:
    counters: dict[str, int] = {}
    exports = []
    for entry in dump:
        card = cast(str, entry["card"])
        counters[card] = counters.get(card, 0) + 1
        exports.append(export_one(entry, counters[card], faces, cards))
    return exports
