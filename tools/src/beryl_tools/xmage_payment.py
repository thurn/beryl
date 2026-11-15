"""The XMage exporter's payments: which mana sources a scenario's cast or activation taps,
as XMage's test commands, from Beryl's trace."""

from __future__ import annotations

from typing import cast

from .card_facts import Cards, Json, Unsupported, made_by
from .xmage_java import java_string


class Payments:
    """The payments of one scenario, each player's in turn: XMage taps its own choice of
    sources for a payment the commands leave to it, which later payments must allow for.
    """

    def __init__(self, cards: Cards) -> None:
        self.cards = cards
        # Players whose earlier payments were left to XMage (`pay`), so their untapped
        # sources may differ from Beryl's when paying another cost.
        self.left_to_xmage: set[str] = set()
        # Whether the last payment tapped each of its sources by name for the mana Beryl
        # made, which says all its "PaymentPlan" answer does.
        self.as_made = False

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
                self.left_to_xmage.add(who)
            return []
        if source in taps:
            taps.remove(source)
        if not taps:
            return []
        if restricted:
            return self.as_made_taps(who, at, taps, mana)
        if self.cards.change_land_types() or any(map(self.cards.mana_choice, taps)):
            if who in self.left_to_xmage:
                raise Unsupported("a payment after one XMage chose the sources of")
            self.left_to_xmage.add(who)
            return []
        lines: list[str] = []
        for name in taps:
            text = self.cards.mana_text(name)
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
        return lines

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
            if text is None:
                raise Unsupported(f"tapping {name} for mana no ability text names")
            lines.append(self.cards.tap_source(at, who, name, text))
            if text.endswith("any color"):
                lines.append(
                    f"setChoice({who}, {java_string(cast(str, made['color']))});"
                )
        self.as_made = True
        return lines
