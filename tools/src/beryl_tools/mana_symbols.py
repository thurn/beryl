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


def made_text(abilities: list[Json], made: Json) -> str | None:
    """The start of the text of the mana ability among `abilities` that made the mana `made`
    (an entry of the trace's `mana`): the one restricted or not as the mana is, whose only
    cost is `{T}`, that makes one color, as `{T}: Add {C}`, or one mana of any color.
    None where no such ability says it."""
    color = cast(str, made["color"])
    made_amount = cast(int, made.get("amount", 1))
    for a in abilities:
        if a["kind"] != "Mana" or bool(a.get("restricted")) != made["restricted"]:
            continue
        if a.get("cost") != {"parts": [{"action": "Tap", "of": "This"}]}:
            continue
        produces = cast(Json, a["produces"])
        colors = cast(list[str], produces["colors"])
        amount = cast(int, produces["amount"])
        if amount != made_amount:
            continue
        if colors == [color]:
            return "{T}: Add " + SYMBOLS[color] * amount
        if len(colors) == len(SYMBOLS) - 1 and color in colors and amount == 1:
            return "{T}: Add one mana of any color"
    return None


def asks_color(abilities: list[Json]) -> bool:
    """Whether a mana ability among `abilities` makes one mana of a color chosen as it
    resolves, which XMage asks (`Payments.as_made_taps` names it)."""
    return any(
        a["kind"] == "Mana"
        and len(cast(list[str], cast(Json, a["produces"])["colors"])) > 1
        and cast(int, cast(Json, a["produces"])["amount"]) == 1
        for a in abilities
    )
