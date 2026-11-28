"""The mana sources Beryl tapped for a payment, as the Forge exporter hands them to the harness."""

from __future__ import annotations

from collections.abc import Sequence
from typing import cast

from .card_facts import Cards, made_by, payment_position, SYMBOLS, Unsupported

JsonObject = dict[str, object]


def mixed_mana(
    actions: Sequence[JsonObject], trace: Sequence[JsonObject], index: int
) -> bool:
    """Whether the payment for the cast or activation at `actions[index]` made mana of more
    than one color, so that which of it Forge's AI spends, and which is left over, is its choice.
    """
    at = payment_position(
        actions, [cast(list[str], t.get("taps", [])) for t in trace], index
    )
    made = [] if at is None else cast(list[JsonObject], trace[at].get("mana", []))
    return len({m["color"] for m in made}) > 1


def taps(
    action: JsonObject,
    actions: Sequence[JsonObject],
    trace: Sequence[JsonObject],
    index: int,
    cards: Cards,
    lands: set[str],
) -> list[str | JsonObject]:
    """The mana sources Beryl tapped to pay for the cast or activation `action`, at
    `actions[index]`, which the harness taps for mana before Forge's AI pays the rest: those
    that only tap for one mana, less the activated card itself. A land with no printed
    mana ability can still tap for one mana when it gains a basic land type.

    A source of restricted mana, as Cavern of Souls, is an object `{"card", "color",
    "restricted"}` with the mana the trace records it made, so that the harness activates
    the same one of its mana abilities for the same color: Forge spends the restricted mana
    only on what its restriction allows, and that spell gets its rider.
    """
    at = payment_position(
        actions, [cast(list[str], t.get("taps", [])) for t in trace], index
    )
    if at is None:
        return []
    found = payment(action, actions, trace, index)
    if any(
        name not in cards.modal_back
        and any(
            a["kind"] == "Mana"
            and bool(cast(JsonObject, a.get("cost", {})).get("mana"))
            for a in cards.abilities(name)
        )
        for name in found
    ):
        raise Unsupported(
            "a payment activating a priced mana ability, which Forge's auto-tapper cannot solve"
        )
    mana = cast(list[JsonObject], trace[at].get("mana", []))[:]
    land_types_change = cards.change_land_types()
    tapped: list[str | JsonObject] = []
    for name in found:
        if cards.restricted_mana(name):
            made = made_by(name, mana)
            color = SYMBOLS[cast(str, made["color"])].strip("{}")
            tapped.append(
                {"card": name, "color": color, "restricted": made["restricted"]}
            )
        elif (
            cards.mana_choice(name)
            and (made := recorded_tap(cards, name, mana)) is not None
        ):
            # A source with several mana abilities, as Svyelunite Temple, which can also be
            # sacrificed for mana: left to Forge's AI it taps other lands than Beryl did, so
            # the harness replays the tap ability the trace recorded.
            color = SYMBOLS[cast(str, made["color"])].strip("{}")
            tapped.append(
                {"card": name, "color": color, "restricted": made["restricted"]}
            )
        elif cards.taps_for_one_mana(name) or (
            land_types_change
            and name in lands
            and not any(a["kind"] == "Mana" for a in cards.abilities(name))
        ):
            tapped.append(name)
    return tapped


def recorded_tap(cards: Cards, name: str, mana: list[JsonObject]) -> JsonObject | None:
    """The trace's `mana` entry for `name` if it names a mana ability whose only cost is
    `{T}`, removing it from `mana` so the harness can activate that ability."""
    for i, m in enumerate(mana):
        if m["source"] == name and cards.made_text(name, m) is not None:
            return mana.pop(i)
    return None


def payment(
    action: JsonObject,
    actions: Sequence[JsonObject],
    trace: Sequence[JsonObject],
    index: int,
) -> list[str]:
    """The names of the permanents Beryl tapped to pay for the cast or activation `action`,
    at `actions[index]`, less the activated card itself."""
    at = payment_position(
        actions, [cast(list[str], t.get("taps", [])) for t in trace], index
    )
    found = [] if at is None else cast(list[str], trace[at].get("taps", []))[:]
    if action["action"] == "Activate" and action["card"] in found:
        found.remove(cast(str, action["card"]))
    return found
