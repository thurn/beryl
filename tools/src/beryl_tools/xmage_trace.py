"""Where each action of a scenario began, as Beryl's trace records it, for the XMage exporter
(`xmage.py`): the turn and step, the open decision, the stack and what the decision took
without asking."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from typing import cast

from .card_facts import Json, Unsupported
from .xmage_java import java_string

# Questions with one answer, which Beryl answers silently, that XMage asks anyway: by kind,
# targets (with none to choose too) and X, whose limit XMage leaves to the payment; by what the
# prompt is for (the trace's `purpose`), the permanents to tap or cards to exile for a cost.
FORCED_ASKED = ("Target", "TargetCount", "X")
FORCED_COSTS = ("crew", "waterbend", "exile-cost", "delve")
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
    # The seat controlling the active player this turn (CR 724.1), if another seat does.
    controller: int | None = None
    # At a copy of a spell's new target, the target it has now, as a pick.
    kept: Json | None = None
    # The mana permanents and spells made, in order: `source`, `color` and `restricted`.
    mana: list[Json] = field(default_factory=list)

    def topmost_spell(self) -> str | None:
        """The name of the highest spell on the stack, if there is one."""
        for item in reversed(self.stack):
            if cast(bool, item["spell"]):
                return cast(str, item["name"])
        return None

    def spell_on_top(self) -> str | None:
        """The name of the top of the stack, if it is a spell."""
        if self.stack and cast(bool, self.stack[-1]["spell"]):
            return cast(str, self.stack[-1]["name"])
        return None

    def check_simultaneous_emblem_triggers(self) -> None:
        """An emblem's triggers on one event, such as Teferi, Hero of Dominaria's for each card
        of a multiple draw, go on the stack together: XMage asks their order, which Beryl, with
        the triggers equivalent (architecture section 7.2), never asks, and an emblem names no
        card to answer with."""
        names = [cast(str, item["name"]) for item in self.stack if not item["spell"]]
        for name in set(names):
            if name.endswith(" Emblem") and names.count(name) > 1:
                raise Unsupported(f"ordering {name}'s simultaneous triggers")

    def new_target(
        self, who: str, pick: Json, player: Callable[[object], str]
    ) -> list[str]:
        """The commands for `who`'s answer `pick` to a copy's new target (`kept` is the target
        it has): XMage asks whether to change each target, then for the new one
        (`StackObjectImpl.chooseNewTarget`). `player` names a seat as XMage's test does.
        """
        changed = pick != self.kept
        lines = [f"setChoice({who}, {'true' if changed else 'false'});"]
        if changed and "card" in pick:
            name = java_string(self.object_name(cast(str, pick["card"])))
            lines.append(f"addTarget({who}, {name});")
        elif changed:
            lines.append(f"addTarget({who}, {player(pick['player'])});")
        return lines

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
    def delves_next(trace: list[Position], index: int) -> bool:
        """Whether the position after `trace[index]` is a choice of the cards delve exiles, or
        Beryl made those choices unasked."""
        after = trace[index + 1 : index + 2]
        return bool(after) and (
            after[0].purpose == "delve"
            or any(f.get("purpose") == "delve" for f in after[0].forced)
        )

    @staticmethod
    def of(t: Json) -> Position:
        """The position one entry of the dump's trace records."""
        return Position(
            turn=cast(int, t["turn"]),
            step=cast(str, t["step"]),
            decider=cast("int | None", t["decider"]),
            controller=cast("int | None", t.get("controller")),
            kind=cast("str | None", t["kind"]),
            purpose=cast("str | None", t.get("purpose")),
            stack=cast(list[Json], t["stack"]),
            flips=cast(list[Json], t["flips"]),
            taps=cast(list[str], t["taps"]),
            mana=cast(list[Json], t.get("mana", [])),
            forced=cast(list[Json], t["forced"]),
            flashback=cast(list[str], t.get("flashback", [])),
            face_down=cast(list[str], t.get("face_down", [])),
            creatures=cast(list[int], t.get("creatures", [1, 1])),
            kept=cast("Json | None", t.get("kept")),
        )
