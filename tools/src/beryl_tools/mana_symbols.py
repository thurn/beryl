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


def has_priced_mana(abilities: list[Json]) -> bool:
    """Whether any mana ability in `abilities` has a mana cost, such as a Signet."""
    return any(
        a["kind"] == "Mana" and bool(cast(Json, a.get("cost", {})).get("mana"))
        for a in abilities
    )


def priced_mana_text(abilities: list[Json], paragraphs: list[str]) -> str | None:
    """The Oracle text prefix of a card's single priced or multi-term mana ability, for XMage's `tapSource`."""
    mana = [a for a in abilities if a["kind"] == "Mana"]
    if len(mana) != 1:
        return None
    produces = cast(Json, mana[0].get("produces", {}))
    has_mana_cost = bool(cast(Json, mana[0].get("cost", {})).get("mana"))
    is_multi_term = cast(int, produces.get("amount", 0)) == 0
    if not (has_mana_cost or is_multi_term):
        return None
    for p in paragraphs:
        if ": Add " in p:
            return p.rstrip(".")
    return None
