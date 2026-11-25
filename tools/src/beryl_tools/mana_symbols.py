"""Beryl's colors of mana as XMage's mana symbols, and the text of a simple mana ability
written with them, which both oracle exporters find a land's mana ability by."""

from __future__ import annotations

from typing import cast

Json = dict[str, object]

# Beryl's colors of mana as XMage's mana symbols.
SYMBOLS: dict[str, str] = {
    "White": "{W}",
    "Blue": "{U}",
    "Black": "{B}",
    "Red": "{R}",
    "Green": "{G}",
    "Colorless": "{C}",
}


def tap_text(ability: Json) -> str | None:
    """The text of a mana ability that costs only tapping its source and makes one kind of
    mana, `{T}: Add` and what it makes, as XMage writes it; None for any other."""
    if ability.get("cost", {}) != {"parts": [{"action": "Tap", "of": "This"}]}:
        return None
    produces = cast(Json, ability["produces"])
    colors = cast(list[str], produces["colors"])
    if len(colors) != 1:
        return None
    return "{T}: Add " + SYMBOLS[colors[0]] * cast(int, produces["amount"])
