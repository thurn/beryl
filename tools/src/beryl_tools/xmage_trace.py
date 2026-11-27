"""Where each action of a scenario began, as Beryl's trace records it, for the XMage exporter
(`xmage.py`): the turn and step, the open decision, the stack and what the decision took
without asking."""

from __future__ import annotations

import re
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

from .card_facts import ESCAPE, Cards, Json, Unsupported, as_their_own
from .xmage_java import Faces, java_string, trample_amount

if TYPE_CHECKING:
    from .xmage import Exporter

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
# The purposes of the picks of creatures a spell's convoke taps, or artifacts its improvise
# taps (CR 702.51a, CR 702.126a).
TAPPED_FOR_A_SPELL = ("convoke", "improvise")


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
    # At a copy of a spell's new target, the target it has now, as a pick.
    kept: Json | None = None
    # The mana permanents and spells made, in order: `source`, `color` and `restricted`.
    mana: list[Json] = field(default_factory=list)
    # The cards in each seat's graveyard before the cast.
    graveyard_cards: list[int] | None = None
    # The open decision chooses what an entering permanent copies.
    enter_as_copy: bool = False
    # The face-down permanents cast as morphs.
    morphed: list[str] = field(default_factory=list)

    damage_choice: Json | None = None

    def supported_castings(self, action: Json) -> int:
        """The casting options, if XMage can offer the cast before payment."""
        costs = cast(int, action["costs"])
        if (
            costs & ESCAPE
            and self.graveyard_cards is not None
            and self.graveyard_cards[cast(int, action["player"])] < 4
        ):
            # The granted escape ability counts three other cards before payment.
            raise Unsupported(
                "escape that needs a card another cost puts in the graveyard"
            )
        return costs

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
    def taps_objects_next(trace: list[Position], index: int) -> bool:
        """Whether the cast at `trace[index]` taps creatures for convoke or artifacts for
        improvise before the next priority, asked or chosen by Beryl unasked."""
        for after in trace[index + 1 :]:
            purposes = [after.purpose, *(f.get("purpose") for f in after.forced)]
            if any(p in TAPPED_FOR_A_SPELL for p in purposes):
                return True
            if after.kind == "Priority":
                return False
        return False

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
        """The position one entry of the dump's trace records. A decision one player answers
        for another who they control is the other's (`as_their_own`)."""
        return Position(
            damage_choice=cast(Json | None, t.get("damage_choice")),
            turn=cast(int, t["turn"]),
            step=cast(str, t["step"]),
            decider=cast("int | None", t.get("decides_for", t["decider"])),
            kind=cast("str | None", t["kind"]),
            purpose=cast("str | None", t.get("purpose")),
            stack=cast(list[Json], t["stack"]),
            flips=cast(list[Json], t["flips"]),
            taps=cast(list[str], t["taps"]),
            mana=cast(list[Json], t.get("mana", [])),
            forced=[
                as_their_own(f, f.get("decides_for"))
                for f in cast(list[Json], t["forced"])
            ],
            flashback=cast(list[str], t.get("flashback", [])),
            face_down=cast(list[str], t.get("face_down", [])),
            creatures=cast(list[int], t.get("creatures", [1, 1])),
            kept=cast("Json | None", t.get("kept")),
            graveyard_cards=cast("list[int] | None", t.get("graveyard_cards")),
            enter_as_copy=cast(bool, t.get("enter_as_copy", False)),
            morphed=cast(list[str], t.get("morphed", [])),
        )


def meld_partner(export: Exporter, p: Position) -> None:
    """Queues, for `export`, the partner XMage's meld asks its controller to pick as the meld ability at
    the top of the stack at `p` resolves, even when there is one (`MeldEffect`), which
    Beryl melds unasked: left unqueued, the pick takes the next card the scenario scripts.
    """
    top = p.stack[-1] if p.stack else None
    if top is None or top["spell"]:
        return
    card = cast(str, top["name"])
    if export.cards.instruction(card, "Meld") is None:
        return
    partners = [
        n for n, shape in export.cards.shapes.items() if shape == "Meld" and n != card
    ]
    if len(partners) == 1:
        who = export.player(top["controller"])
        export.emit(f"setChoice({who}, {java_string(partners[0])});")


def resolution_skips(
    cards: Cards,
    before: Position,
    following: Sequence[Position],
    actions: Sequence[Json],
    at: str,
    player: Callable[[object], str],
) -> Iterable[str]:
    """Supply declines that XMage can ask for even when Beryl has no choice to offer."""
    if not before.stack:
        return
    top = before.stack[-1]
    name = cast(str, top["name"])
    if cards.instruction(name, "Wish") is not None and (
        not following or following[0].purpose != "wish"
    ):
        yield f'setOptionalChoice({player(top["controller"])}, {at}, "No");'
    if not cards.search_by_each_player(name):
        return
    if actions and actions[0].get("kind") == "ChooseUpTo":
        return
    if not following:
        raise Unsupported("a search by each player who lost objects, at the end")
    for seat, (old_count, count) in enumerate(
        zip(before.creatures, following[0].creatures)
    ):
        if count < old_count:
            yield f"addTarget({player(seat)}, TestPlayer.TARGET_SKIP);"


def cast_targets(
    actions: Sequence[Json], trace: Sequence[Position], index: int
) -> list[Json]:
    """The target choices following this cast, or the choices Beryl made without asking."""
    answers: list[Json] = []
    for action in actions[index + 1 :]:
        if action["action"] != "Answer" or action["kind"] not in (
            "Target",
            "TargetCount",
        ):
            break
        answers.append(action)
    if not answers and index + 1 < len(trace):
        for forced in trace[index + 1].forced:
            if forced["kind"] != "Target":
                break
            answers.append(forced)
    return answers


class Combat:
    def __init__(self, faces: Faces) -> None:
        self.faces = faces
        self.attackers: dict[int, list[str]] = {}
        self.targets: dict[int, dict[str, str]] = {}
        self.members: dict[int, dict[str, list[str]]] = {}
        self.blocks: dict[int, dict[str, str]] = {}
        self.trampled: set[int] = set()

    def has_banding(self, card: str) -> bool:
        return bool(re.search(r"^Banding\b", self.faces.oracle.get(card, ""), re.M))

    def answer(self, export: Exporter, action: Json, p: Position) -> None:
        pick = cast(Json, action["pick"])
        kind, what = action["kind"], pick["pick"]
        if kind in ("Attacker", "Blocker") and what == "Done":
            return
        who = export.player(action["player"])
        if kind == "Attacker" and what in ("Card", "Band"):
            self.attack(
                export,
                p,
                who,
                cast(str, pick["card"]),
                cast(str | None, pick.get("first")),
            )
        elif kind == "Blocker" and what == "Card":
            self.block(
                export, p, who, cast(str, pick["card"]), cast(int, action["player"])
            )
        elif kind == "DamageDivision" and what == "Number" and p.damage_choice:
            self.damage(export, p, who, cast(int, pick["n"]))
        elif kind == "AttackTarget":
            raise Unsupported(
                "choosing what a creature put onto the battlefield attacks"
            )
        else:
            raise Unsupported(f"answering {kind} with {what}")

    def attack(
        self, export: Exporter, p: Position, who: str, card: str, first: str | None
    ) -> None:
        """XMage schedules individual attacks, then asks banders about joining same-target attackers."""
        targets = self.targets.setdefault(p.turn, {})
        self.attackers.setdefault(p.turn, []).append(card)
        if first is not None:
            defender = targets[first]
        else:
            defender = "playerB" if who == "playerA" else "playerA"
            following = export.actions[export.index + 1 : export.index + 2]
            if following and following[0].get("kind") == "AttackTarget":
                target = cast(Json, following[0]["pick"])
                export.taken.add(id(following[0]))
                if target["pick"] != "DefendingPlayer":
                    defender = java_string(cast(str, target["card"]))
        targets[card] = defender
        export.emit(
            f"attack({export.turn(p.turn)}, {who}, {java_string(card)}, {defender});"
        )

        if any(
            later.turn == p.turn
            and later.step == p.step
            and a.get("kind") == "Attacker"
            for a, later in zip(
                export.actions[export.index + 1 :], export.trace[export.index + 1 :]
            )
        ):
            return
        bands: dict[str, list[str]] = {}
        for a, later in zip(export.actions[: export.index + 1], export.trace):
            if (
                later.turn == p.turn
                and later.step == p.step
                and a.get("kind") == "Attacker"
            ):
                choice = cast(Json, a["pick"])
                if choice["pick"] == "Band":
                    chosen = cast(str, choice["card"])
                    leader = cast(str | None, choice.get("first"))
                    if leader is None:
                        bands[chosen] = [chosen]
                    else:
                        bands[leader].append(chosen)
        if bands and len(self.attackers[p.turn]) != len(targets):
            raise Unsupported("two attackers with the same name forming a band")
        self.members[p.turn] = bands
        grouped: set[str] = set()
        for attacker, target in targets.items():
            if attacker in grouped or not self.has_banding(attacker):
                continue
            partners = bands.get(attacker, [attacker])[1:]
            others = {
                name
                for name, to in targets.items()
                if name != attacker and to == target
            }
            for member in partners:
                export.emit(f"setChoice({who}, true);")
                export.emit(f"setChoice({who}, {java_string(member)});")
                others.remove(member)
            if not any(not self.has_banding(member) for member in partners):
                eligible = others - grouped
            else:
                eligible = {
                    member for member in others - grouped if self.has_banding(member)
                }
            if eligible:
                export.emit(f"setChoice({who}, false);")
            grouped.update(partners)

    def block(
        self, export: Exporter, p: Position, who: str, card: str, seat: int
    ) -> None:
        attackers = self.attackers.get(p.turn, [])
        if len(attackers) == 1:
            attacker = attackers[0]
        elif self.members.get(p.turn):
            i = export.index + 1
            following = export.actions[i]
            if following.get("kind") == "BlockTarget":
                export.taken.add(id(following))
                selected = cast(Json, following["pick"])
            else:
                selected = next(
                    (
                        cast(Json, a["pick"])
                        for a in export.trace[i].forced
                        if a["kind"] == "BlockTarget"
                    ),
                    {},
                )
            attacker = cast(str, selected.get("card"))
            if not attacker:
                raise Unsupported("a band member chosen as a block target")
        else:
            raise Unsupported(f"a block among {len(attackers)} declared attackers")
        blocks = self.blocks.setdefault(p.turn, {})
        blocks[card] = attacker
        export.emit(
            f"block({export.turn(p.turn)}, {who}, {java_string(card)}, {java_string(attacker)});"
        )
        amount = trample_amount(self.faces, attacker, card)
        has_exact_pick = any(
            later.turn == p.turn
            and later.damage_choice
            and later.damage_choice["source"] == attacker
            for later in export.trace[export.index + 1 :]
        )
        if amount is not None and not has_exact_pick:
            if p.turn in self.trampled:
                raise Unsupported("a trampler blocked by more than one creature")
            self.trampled.add(p.turn)
            export.emit(f"setChoiceAmount({export.player(1 - seat)}, {amount});")

    def damage(self, export: Exporter, p: Position, who: str, amount: int) -> None:
        """XMage takes one list per source. Infer the last creature's share for two-way divisions."""
        choice = p.damage_choice
        assert choice is not None
        source = cast(str, choice["source"])
        first = cast(str, choice["recipient"])
        blocks = self.blocks.get(p.turn, {})
        if (
            source in self.targets.get(p.turn, {})
            and sum(
                bool(re.search(r"\btrample\b", self.faces.oracle.get(card, ""), re.I))
                for card in self.targets[p.turn]
            )
            > 1
        ):
            raise Unsupported(
                "trample assignments that depend on another trampler's choice"
            )
        if choice["recipients"] == 1:
            amounts = [amount]
        elif choice["recipients"] == 2:
            if source in blocks:
                target = blocks[source]
                group = next(
                    (
                        members
                        for members in self.members.get(p.turn, {}).values()
                        if target in members
                    ),
                    [target],
                )
                order = [name for name in self.targets[p.turn] if name in group]
            else:
                order = [name for name, target in blocks.items() if target == source]
            if len(order) != 2 or first not in order:
                raise Unsupported(
                    "an exact damage share with ambiguous combat recipients"
                )
            other = cast(int, choice["remaining"]) - amount
            amounts = [amount if name == first else other for name in order]
        else:
            raise Unsupported("exact combat damage among more than two creatures")
        joined = ", ".join(map(str, amounts))
        export.emit(f"setChoiceAmount({who}, {joined});")
