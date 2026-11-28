"""Oracle scenarios that pay with an outlet: a mana ability whose cost sacrifices or taps a
permanent its controller chooses, as Krark-Clan Ironworks's "Sacrifice an artifact: Add {C}{C}".
Beryl activates one in the middle of a payment without asking when it is the one way to pay, so
a scenario's actions do not say it, Forge's AI pays a cost without such abilities, and XMage
asks which permanent to sacrifice. Both exporters refuse such a scenario."""

from __future__ import annotations

from collections.abc import Sequence
from typing import cast

from .card_facts import Cards, Json, Unsupported


def refuse_outlet_payments(cards: Cards, setup: Json, actions: Sequence[Json]) -> None:
    """Refuses a scenario that casts a spell while a permanent on the battlefield has an
    outlet."""
    if not any(a.get("action") == "Cast" for a in actions):
        return
    for player in cast(list[Json], setup["players"]):
        for permanent in cast(list[Json], player["battlefield"]):
            if has_outlet(cards, cast(str, permanent["card"])):
                raise Unsupported("a payment that may activate an outlet mana ability")


def has_outlet(cards: Cards, name: str) -> bool:
    """Whether a face of `name` has a mana ability whose cost chooses a permanent."""
    for ability in cards.abilities(name):
        if ability.get("kind") != "Mana":
            continue
        cost = cast(Json, ability.get("cost", {}))
        for part in cast(list[Json], cost.get("parts", [])):
            if part.get("of") == "Chosen" and part.get("zone") == "Battlefield":
                return True
    return False
