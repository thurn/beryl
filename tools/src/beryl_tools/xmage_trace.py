"""Where each action of a scenario began, as Beryl's trace records it, for the XMage exporter
(`xmage.py`): the turn and step, the open decision, the stack and what the decision took
without asking."""

from __future__ import annotations

from dataclasses import dataclass
from typing import cast

from .card_facts import Json, Unsupported

# Questions with one answer, which Beryl answers silently, that XMage asks anyway: by kind,
# targets (with none to choose too) and X, whose limit XMage leaves to the payment; by what the
# prompt is for (the trace's `purpose`), the permanents to tap or cards to exile for a cost.
FORCED_ASKED = ("Target", "TargetCount", "X")
FORCED_COSTS = ("crew", "waterbend", "exile-cost")
# Others XMage asks when the one answer stands for several identical cards.
FORCED_CHOICES = ("ChooseObject", "ChooseUpTo")
# What the prompts are for whose cards XMage asks as targets even when they are identical, as
# Brainstorm's cards to put back from a hand of Islands: their answers go in the target queue.
FORCED_TARGETS = ("put-back",)


@dataclass
class Position:
    turn: int
    step: str
    decider: int | None
    kind: str | None
    # What the open decision's prompt is for: a kind says only the form of its answer.
    purpose: str | None
    stack: list[Json]
    flips: list[Json]
    taps: list[str]
    forced: list[Json]
    flashback: list[str]
    face_down: list[str]
    # How many creatures each seat controls.
    creatures: list[int]

    def object_name(self, card: str) -> str:
        """How XMage's test commands name the object `card` picks: a face-down permanent, which
        has no name, as XMage's stand-in for one."""
        if card in self.face_down:
            return "[face_down_creature]"
        return card

    def stack_ability(self, source: str) -> str:
        """The name XMage's test player targets the ability of `source` on the stack by: a
        stack ability's name is "stack ability (" and its rule, which Beryl does not know, so
        only the prefix names it, and only while it is the one ability on the stack."""
        abilities = [
            cast(str, i["name"]) for i in self.stack if not cast(bool, i["spell"])
        ]
        if abilities != [source]:
            raise Unsupported("a target among several abilities on the stack")
        return "stack ability ("

    @staticmethod
    def of(t: Json) -> Position:
        """The position one entry of the dump's trace records."""
        return Position(
            turn=cast(int, t["turn"]),
            step=cast(str, t["step"]),
            decider=cast("int | None", t["decider"]),
            kind=cast("str | None", t["kind"]),
            purpose=cast("str | None", t.get("purpose")),
            stack=cast(list[Json], t["stack"]),
            flips=cast(list[Json], t["flips"]),
            taps=cast(list[str], t["taps"]),
            forced=cast(list[Json], t["forced"]),
            flashback=cast(list[str], t.get("flashback", [])),
            face_down=cast(list[str], t.get("face_down", [])),
            creatures=cast(list[int], t.get("creatures", [1, 1])),
        )
