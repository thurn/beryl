"""The XMage exporter's payments: which mana sources a scenario's cast or activation taps,
as XMage's test commands, from Beryl's trace."""

from __future__ import annotations

from typing import cast

from .card_facts import Cards, Json, Unsupported, made_by
from .mana_symbols import has_priced_mana, priced_mana_text
from .xmage_java import java_string
from .xmage_trace import Position


class Payments:
    """The payments of one scenario, each player's in turn: XMage taps its own choice of
    sources for a payment the commands leave to it, which later payments must allow for.
    """

    def __init__(
        self, cards: Cards, player_cards: dict[str, set[str]] | None = None
    ) -> None:
        self.cards = cards
        self.player_cards: dict[str, set[str]] = player_cards or {}
        # Players whose earlier payments were left to XMage (`pay`), so their untapped
        # sources may differ from Beryl's when paying another cost.
        self.left_to_xmage: set[str] = set()
        # Whether the last payment tapped each of its sources by name for the mana Beryl
        # made, which says all its "PaymentPlan" answer does.
        self.as_made = False
        # Where to float mana for a cast offered by this resolution.
        self.resolve_at: tuple[int, str, int] | None = None

    def float_for_offer(
        self,
        who: str,
        at: str,
        offer: Position,
        paid: Position | None,
        lines: list[str],
    ) -> None:
        """Float recorded mana in the same step before the resolving object offers the cast."""
        self.as_made = False
        if paid is None or not paid.mana:
            return
        resolve_at = self.resolve_at
        if resolve_at is None or resolve_at[:2] != (offer.turn, offer.step):
            raise Unsupported(
                "paying for an offered cast outside a scripted resolution"
            )
        commands = self.pay(who, at, False, list(paid.taps), list(paid.mana), None)
        before = resolve_at[2]
        lines[before:before] = commands
        self.resolve_at = (offer.turn, offer.step, before + len(commands))

    def pay(
        self,
        who: str,
        at: str,
        stack: bool,
        taps: list[str],
        mana: list[Json],
        source: str | None,
    ) -> list[str]:
        """The commands that replay `who`'s payment at `at` (XMage's turn and step), which
        tapped `taps` and made `mana`, by each source's name and mana ability, excluding
        `source`, an activated ability's own tap. XMage pays when the stack is occupied
        (`stack`) or a source can produce several kinds of mana. Sacrificing a source for
        mana of any color needs a choice the trace doesn't record. A payment with a source
        of restricted mana names each source's ability and color (`as_made_taps`).
        """
        self.as_made = False
        if any(map(self.cards.sacrificed_for_mana_of_any_color, taps)):
            raise Unsupported("a payment with mana whose color XMage asks")
        restricted = any(map(self.cards.restricted_mana, taps))
        if stack and restricted:
            raise Unsupported("a payment with restricted mana onto a stack")
        if stack:
            if who in self.left_to_xmage and taps:
                raise Unsupported(
                    "a payment after one XMage chose the sources of, onto a stack"
                )
            # XMage chooses these sources, so a later payment may find one already tapped.
            if taps:
                self.require_no_mana_type_prompt(who)
                self.left_to_xmage.add(who)
            return []
        if source in taps:
            taps.remove(source)
        if not taps:
            return []
        if restricted:
            return self.as_made_taps(who, at, taps, mana)
        grants_mana = any(
            g["kind"] == "Mana" for _, gs in self.cards.grants for g in gs
        )
        if (
            not self.cards.change_land_types()
            and who not in self.left_to_xmage
            and (
                any(
                    self.cards.mana_choice(t) or _any_type_mana(self.cards, t)
                    for t in taps
                )
                or grants_mana
            )
        ):
            try:
                return self.as_made_taps(who, at, taps, list(mana))
            except Unsupported:
                pass
        priced_count = sum(
            1
            for name in taps
            if name not in self.cards.modal_back
            and has_priced_mana(self.cards.abilities(name))
        )
        if priced_count >= 2:
            raise Unsupported(
                "a payment chaining several priced mana abilities, which XMage cannot solve"
            )
        if self.cards.change_land_types() or any(map(self.cards.mana_choice, taps)):
            if who in self.left_to_xmage:
                raise Unsupported("a payment after one XMage chose the sources of")
            self.require_no_mana_type_prompt(who)
            self.left_to_xmage.add(who)
            return []
        lines: list[str] = []
        for name in taps:
            text = self.cards.mana_text(name)
            if text is None and name not in self.cards.modal_back:
                text = priced_mana_text(
                    self.cards.abilities(name), self.cards.paragraphs(name)
                )
            if text is not None:
                if who in self.left_to_xmage:
                    # XMage paid an earlier spell with sources of its own choosing, and may
                    # have tapped the one Beryl tapped here, so the command would fail or not
                    # depending on which it chose.
                    raise Unsupported(
                        "a payment tapping a source by name after one XMage chose the "
                        "sources of"
                    )
                lines.append(self.cards.tap_source(at, who, name, text))
                for choice in _extra_type_choices(name, text, mana):
                    lines.append(f"setChoice({who}, {java_string(choice)});")
        return lines

    def require_no_mana_type_prompt(self, who: str) -> None:
        """Refuses a payment XMage chooses the sources of when `who` has a source that asks a
        mana type. XMage taps whichever sources it picks, which varies from run to run. If it
        picks that one, the script has no choice to answer the prompt, and the next queued
        choice answers it instead, failing the scenario on some runs."""
        if self.asks_mana_type(who):
            raise Unsupported(
                "a payment XMage chooses the sources of, with a source that asks "
                "for a mana type"
            )

    def asks_mana_type(self, who: str) -> bool:
        """Whether `who` has a source that XMage asks a mana type of when it taps the source
        itself (one making mana of any type another land could produce, with two or more
        types among `who`'s lands). A payment XMage chooses the sources of can tap it, and
        the script has no choice to answer the prompt."""
        names = self.player_cards.get(who, set())
        return any(_any_type_mana(self.cards, n) for n in names) and (
            _multiple_land_mana_types(self.cards, names)
        )

    def as_made_taps(
        self, who: str, at: str, taps: list[str], mana: list[Json]
    ) -> list[str]:
        """The commands that tap each of `taps` for the mana the trace records it made, among
        `mana`: its mana ability by text, and the color of mana of any color, which XMage
        asks. Left to XMage, a payment could tap Cavern of Souls for colorless mana where
        Beryl spent its restricted mana, and the spell could be countered."""
        if who in self.left_to_xmage or self.cards.change_land_types():
            raise Unsupported(
                "a payment with restricted mana after one XMage chose the sources of"
            )
        lines: list[str] = []
        for name in taps:
            made = made_by(name, mana)
            text = self.cards.made_text(name, made)
            if text is None and _any_type_mana(self.cards, name):
                text = "{T}: Add one mana of any type"
            if text is None:
                raise Unsupported(f"tapping {name} for mana no ability text names")
            lines.append(self.cards.tap_source(at, who, name, text))
            if text.endswith("any color") or (
                text.endswith("any type")
                and _multiple_land_mana_types(
                    self.cards, self.player_cards.get(who, set())
                )
            ):
                lines.append(
                    f"setChoice({who}, {java_string(cast(str, made['color']))});"
                )
        self.as_made = True
        return lines


def _any_type_mana(cards: Cards, source: str) -> bool:
    """Whether `source` has a mana ability making mana of any type another permanent could
    produce, whose color XMage asks with a choice prompt when two or more types exist.
    """
    return any("any type that" in p for p in cards.paragraphs(source))


def _multiple_land_mana_types(cards: Cards, names: set[str]) -> bool:
    """Whether the cards in `names` together have mana abilities producing more than one
    type of mana (so XMage prompts for a mana type choice on an `_any_type_mana` land).
    """
    colors: set[str] = set()
    for name in names:
        for a in cards.abilities(name):
            if a["kind"] != "Mana":
                continue
            produces = cast(Json, a.get("produces", {}))
            colors.update(cast(list[str], produces.get("colors", [])))
        for p in cards.paragraphs(name):
            if "{T}: Add " in p:
                for sym, col in _MANA_SYMBOLS.items():
                    if sym in p:
                        colors.add(col)
    return len(colors) > 1


_MANA_SYMBOLS = {
    "{W}": "White",
    "{U}": "Blue",
    "{B}": "Black",
    "{R}": "Red",
    "{G}": "Green",
    "{C}": "Colorless",
}


def _extra_type_choices(name: str, text: str, mana: list[Json]) -> list[str]:
    """Colors chosen for an `Added::TypeProduced` trigger (e.g. Mana Flare) when tapping a
    permanent whose mana ability produces more than one type of mana (e.g. `{T}: Add {W}{U}`).
    """
    _, _, tail = text.partition("Add ")
    printed: dict[str, int] = {}
    for sym, color in _MANA_SYMBOLS.items():
        count = tail.count(sym)
        if count > 0:
            printed[color] = count
    if len(printed) <= 1:
        return []
    produced: dict[str, int] = {}
    for entry in mana:
        if entry.get("source") == name:
            color = cast(str, entry["color"])
            produced[color] = produced.get(color, 0) + cast(int, entry.get("amount", 1))
    choices: list[str] = []
    for color, base in printed.items():
        extra = produced.get(color, 0) - base
        if extra > 0:
            choices.extend([color] * extra)
    return choices
