"""Where each action of a scenario began, as Beryl's trace records it, for the XMage exporter
(`xmage.py`): the turn and step, the open decision, the stack and what the decision took
without asking."""

from __future__ import annotations

from dataclasses import dataclass
from typing import cast

from .card_facts import Json


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
