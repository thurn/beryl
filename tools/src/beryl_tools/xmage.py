"""The XMage exporter: Beryl's scenarios as XMage JUnit tests (docs/engine-architecture.md,
section 17.2).

`beryl_scenario dump` writes every scenario with a trace: the turn, step, open decision, stack
and coin flips each action began from, as Beryl played it. Beryl scripts decision by decision;
XMage's `CardTestPlayerBase` scripts commands by turn and step, run on a player's priority in
queue order, with the answers to questions in per-player queues (`addTarget`, `setChoice`). The
trace is what places each Beryl action as XMage commands:

- The setup's active player is playerA, and the setup's turn is XMage's turn 1, so the seats'
  draws line up. Libraries are exactly the setup's (`removeAllCardsFromLibrary`).
- A command at a position with an empty stack first waits for the stack to resolve
  (`waitStackResolved`); one at a position with a spell on the stack waits for that spell
  (`spellOnStack`), and one answering an ability with no spell on the stack waits for no
  spell to be there (`!spellOnStack`), or it would answer the spell that made the ability, such
  as Portable Hole's trigger. `Resolve`, and passing priority over a non-empty stack, let one object
  resolve (`resolveTop`). A command after the other player put something on an empty stack
  first passes priority (`passPriority`), or it would run before that.
- Answers go to the answer queues in Beryl's order, with the questions of one answer that Beryl
  answered without asking (the trace's `forced`): as plain answers where XMage always asks
  them, as optional ones (`setOptionalChoice`) where it may not. Coin flips replay Beryl's
  results, and mana payments replay the sources Beryl tapped. Where XMage asks a card's
  questions differently, rules over the dump's description of each card the scenario names
  (`card_facts.py`) translate them.
- Assertions run inside `runCode` at their position, through XMage's `assert*` family where it
  states the same fact and `BerylScenarioTest`'s helpers where it does not. Beryl's decision
  assertions (who decides, among how many candidates) are about Beryl's decision surface and are
  skipped, as are assertions in the middle of a decision, where XMage has no priority to run
  them.

A scenario or card outside this vocabulary is reported as unsupported, with the reason.
"""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from typing import cast

from .card_facts import Cards, Json, Unsupported, payment_taps
from .xmage_java import (
    EVERY_TURN,
    ORDER,
    STEPS,
    COUNTERS,
    Export,
    Faces,
    assertion_code,
    java_string,
    method_name,
    trample_amount,
)

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

# The answers XMage takes from the choice queue, by Beryl decision kind: with these, a
# waterbend's permanents, which Beryl asks as a payment whose purpose says so.
CHOICE_KINDS = ("ChooseUpTo", "ChooseObject", "TriggerOrder")
YES_NO_KINDS = ("YesNo", "CastOffer")


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


class Exporter:
    """Translates one scenario into the statements of a test method."""

    def __init__(self, entry: Json, faces: Faces, export: Export) -> None:
        self.entry = entry
        self.faces = faces
        self.export = export
        self.cards = Cards(cast(Json, entry.get("cards", {})), faces.oracle)
        setup = cast(Json, entry["setup"])
        self.setup: Json = setup
        self.first_turn: int = cast(int, setup["turn"])
        self.active: int = cast(int, setup["active"])
        self.lines: list[str] = export.body
        # The creatures declared as attackers this turn, for blocks.
        self.attackers: dict[int, list[str]] = {}
        self.last: tuple[int, int] = (1, ORDER.index(cast(str, setup["step"])))
        # The players who must pass before their next command at an empty stack in this step
        # (`prepare`).
        self.step: tuple[int, str] = (0, "")
        self.waiting: set[str] = set()
        # The scenario as `run` walks it: the actions, their positions, the index of the one at
        # hand, and the answers an earlier command already took (by `id`).
        self.actions: list[Json] = cast(list[Json], entry["actions"])
        self.trace: list[Position] = []
        self.index = 0
        self.taken: set[int] = set()
        # Whether an earlier payment was left to XMage (`pay`), which may have tapped any
        # untapped source, so a later payment cannot tap one by name.
        self.payment_left_to_xmage = False
        # The resolving object and kind of the last answer, for XMage's untap shortcut.
        self.last_answer: tuple[str | None, str] = (None, "")
        # After a yes to a `Cards.may_choose` ability's question: the command for the card chosen.
        self.may_choose: str | None = None
        # A scry in progress: who scries, the cards for the bottom, and those ordered on top.
        self.scry: tuple[str, list[str], list[str]] | None = None
        # A `Cards.takes` card cast but not yet resolved by its picks, and the cards chosen for the
        # hand by the one resolving.
        self.take_pending: str | None = None
        self.taking: list[str] | None = None
        # The `Cards.unless_pays` counterspells on the stack whose payer Beryl has not asked yet, with
        # that payer.
        self.unless_pays: dict[str, str] = {}
        # The turns in which a trampler was blocked (`xmage_java.trample_amount`).
        self.trampled: set[int] = set()

    def player(self, seat: object) -> str:
        return "playerA" if seat == self.active else "playerB"

    def turn(self, turn: int) -> int:
        return turn - self.first_turn + 1

    def at(self, p: Position) -> str:
        """`turn, PhaseStep.STEP` for a position."""
        return f"{self.turn(p.turn)}, PhaseStep.{STEPS[p.step]}"

    def emit(self, line: str) -> None:
        self.lines.append(line)

    def reach(self, turn: int, step: str) -> None:
        self.last = max(self.last, (self.turn(turn), ORDER.index(step)))

    def prepare(self, who: str, p: Position, starts: bool = False) -> None:
        """Before a priority command of `who` at `p`. At an empty stack the command first waits
        for the stack to empty, and first passes priority if the other player has since put
        something on an empty stack: XMage runs a command whenever its player holds priority,
        which `who` also did before that. `starts` says this command puts something on the
        stack."""
        step = (p.turn, p.step)
        if step != self.step:
            self.step = step
            self.waiting.clear()
        if not p.stack:
            if who in self.waiting:
                self.emit(f"passPriority({self.at(p)}, {who});")
            self.emit(f"waitStackResolved({self.at(p)}, {who});")
        self.waiting.discard(who)
        if starts and not p.stack:
            self.waiting.add("playerB" if who == "playerA" else "playerA")

    def spell_on_top(self, p: Position) -> str | None:
        if p.stack and cast(bool, p.stack[-1]["spell"]):
            return cast(str, p.stack[-1]["name"])
        return None

    def run(self) -> None:
        self.arrange()
        actions = self.actions
        trace = [
            self.position(cast(Json, t))
            for t in cast(list[object], self.entry["trace"])
        ]
        self.trace = trace
        for i, (action, p) in enumerate(zip(actions, trace)):
            self.index = i
            self.unasked(p)
            for answer in p.forced:
                self.forced(answer, p)
            for flip in p.flips:
                won = "true" if flip["won"] else "false"
                self.emit(f"setFlipCoinResult({self.player(flip['player'])}, {won});")
            self.reach(p.turn, p.step)
            if action["action"] != "Answer":
                self.end_scry()
            self.simultaneous_emblem_triggers(p)
            self.act(action, p)
        self.end_scry()
        if self.take_pending is not None:
            raise Unsupported(f"{self.take_pending} putting cards into a hand unasked")
        self.stop()

    @staticmethod
    def simultaneous_emblem_triggers(p: Position) -> None:
        """An emblem's triggers on one event, such as Teferi, Hero of Dominaria's for each card
        of a multiple draw, go on the stack together: XMage asks their order, which Beryl, with
        the triggers equivalent (architecture section 7.2), never asks, and an emblem names no
        card to answer with."""
        names = [cast(str, item["name"]) for item in p.stack if not item["spell"]]
        for name in set(names):
            if name.endswith(" Emblem") and names.count(name) > 1:
                raise Unsupported(f"ordering {name}'s simultaneous triggers")

    def unasked(self, p: Position) -> None:
        """For each `Cards.unless_pays` counterspell that left the stack without Beryl asking its
        payer, the no XMage asks for (optional, should it not ask)."""
        on_stack = {cast(str, item["name"]) for item in p.stack}
        for card, payer in list(self.unless_pays.items()):
            if card not in on_stack:
                del self.unless_pays[card]
                self.emit(f'setOptionalChoice({payer}, {self.at(p)}, "No");')

    def forced(self, answer: Json, p: Position) -> None:
        """A question with one answer that Beryl answered silently: queued like a scripted
        answer where XMage asks it too, optional where it may not."""
        kind = answer["kind"]
        scrying = self.scry is not None and kind == "ChooseObject"
        asked = kind in FORCED_ASKED or answer.get("purpose") in FORCED_COSTS
        if asked or self.may_choose is not None or scrying:
            self.answer(answer, p)
            return
        pick = cast(Json, answer["pick"])
        if (
            kind in FORCED_CHOICES
            and "card" in pick
            and answer.get("purpose") in (FORCED_TARGETS)
        ):
            who = self.player(answer["player"])
            self.emit(f"addTarget({who}, {java_string(cast(str, pick['card']))});")
            return
        if kind in FORCED_CHOICES and "card" in pick:
            who = self.player(answer["player"])
            turn = self.turn(cast(int, answer["turn"]))
            step = STEPS[cast(str, answer["step"])]
            card = java_string(cast(str, pick["card"]))
            self.emit(f"setOptionalChoice({who}, {turn}, PhaseStep.{step}, {card});")

    def end_scry(self) -> None:
        """Ends a scry. XMage asks the order of two or more cards left on top bottom up, first
        the one Beryl puts in place unasked, so the scenario cannot name that answer."""
        if self.scry is not None and self.scry[2]:
            raise Unsupported("ordering a scry's cards on top")
        self.scry = None

    @staticmethod
    def position(t: Json) -> Position:
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

    def arrange(self) -> None:
        self.emit("removeAllCardsFromLibrary(playerA);")
        self.emit("removeAllCardsFromLibrary(playerB);")
        self.emit("skipInitShuffling();")
        # XMage's shared random source starts unseeded, so a shuffle during the scenario (such
        # as Timetwister's) drew different cards on each run. Seeding it with the scenario's seed
        # makes every run play the same game, as Forge's harness already does.
        self.emit(f"mage.util.RandomUtil.setSeed({cast(int, self.setup['seed'])}L);")
        start = cast(str, self.setup["step"])
        for seat, setup in enumerate(cast(list[Json], self.setup["players"])):
            who = self.player(seat)
            life = cast(int, setup["life"])
            if life != 20:
                self.emit(f"setLife({who}, {life});")
            for name in cast(list[str], setup["hand"]):
                self.emit(f"addCard(Zone.HAND, {who}, {java_string(name)});")
            for permanent in cast(list[Json], setup["battlefield"]):
                self.permanent(who, permanent, start)
            for name in reversed(cast(list[str], setup["graveyard"])):
                self.emit(f"addCard(Zone.GRAVEYARD, {who}, {java_string(name)});")
            for name in cast(list[str], setup["exile"]):
                self.emit(f"addCard(Zone.EXILED, {who}, {java_string(name)});")
            # The last card added is the top one.
            for name in reversed(cast(list[str], setup["library"])):
                self.emit(f"addCard(Zone.LIBRARY, {who}, {java_string(name)});")
            # XMage plays its turn 1 from the untap step, where Beryl's setup starts at its step
            # with the earlier ones done: a permanent that flips a coin at the upkeep flips one
            # XMage would lose half the time. Beryl's setup skipped that flip, so XMage wins it.
            flips = any(
                self.cards.flips_a_coin_each_turn(cast(str, p["card"]))
                for p in cast(list[Json], setup["battlefield"])
            )
            if (
                flips
                and who == "playerA"
                and ORDER.index(start) > ORDER.index("Upkeep")
            ):
                self.emit(f"setFlipCoinResult({who}, true);")
        self.emit("setStrictChooseMode(true);")

    def permanent(self, who: str, permanent: Json, start: str) -> None:
        card = cast(str, permanent["card"])
        name = self.faces.back[card] if permanent["face"] else card
        tapped = "true" if permanent["tapped"] else "false"
        self.emit(
            f"addCard(Zone.BATTLEFIELD, {who}, {java_string(name)}, 1, {tapped});"
        )
        counters = cast("Json | None", permanent["counters"])
        if counters is None:
            return
        kind = cast(str, counters["kind"])
        count = cast(int, counters["count"])
        if kind == "Loyalty":
            # XMage puts a planeswalker onto the battlefield with its printed loyalty.
            count -= self.faces.loyalty.get(name, 0)
        if count < 0:
            raise Unsupported(f"{name} starts with fewer counters than it enters with")
        if count:
            self.emit(
                f"addCounters(1, PhaseStep.{STEPS[start]}, {who}, {java_string(name)}, "
                f"CounterType.{COUNTERS[kind]}, {count});"
            )

    def act(self, action: Json, p: Position) -> None:
        kind = cast(str, action["action"])
        if kind == "Assert":
            self.check(cast(Json, action["assertion"]), p)
        elif kind == "AdvanceTo":
            self.reach(cast(int, action["turn"]), cast(str, action["step"]))
        elif kind == "Resolve":
            if p.decider is None:
                raise Unsupported("resolves after the game is over")
            who = self.player(p.decider)
            self.prepare(who, p)
            self.emit(f"resolveTop({self.at(p)}, {who});")
            self.empty_search(p)
        elif kind == "PlayLand":
            who = self.player(action["player"])
            card = cast(str, action["card"])
            self.prepare(who, p)
            name = self.faces.land_back.get(card, card)
            self.emit(f"playLand({self.at(p)}, {who}, {java_string(name)});")
        elif kind == "Cast":
            self.cast(action, p)
        elif kind == "Activate":
            self.activate(action, p)
        elif kind == "Answer":
            self.answer(action, p)
        elif kind == "TurnFaceUp":
            self.turn_face_up(action, p)
        else:
            raise Unsupported(f"action {kind}")

    def turn_face_up(self, action: Json, p: Position) -> None:
        """Turns a manifested permanent face up for its card's mana cost, the special action
        XMage offers as "<cost>: Turn this face-down permanent face up"."""
        who = self.player(action["player"])
        card = cast(str, action["card"])
        self.prepare(who, p)
        self.pay(who, p, None)
        text = java_string(f"{self.faces.mana_cost[card]}: Turn")
        self.emit(f"activateAbility({self.at(p)}, {who}, {text});")

    @staticmethod
    def object_name(card: str, p: Position) -> str:
        """How XMage's test commands name the object `card` picks: a face-down permanent, which
        has no name, as XMage's stand-in for one."""
        if card in p.face_down:
            return "[face_down_creature]"
        return card

    def empty_search(self, p: Position) -> None:
        """After a spell or ability resolves whose search by each player who lost objects to it
        Beryl did not ask (`Cards.search_by_each_player`), the skip XMage's search with nothing
        to find asks of each of them: the players whose creatures the next position has fewer
        of. One who lost none searches for no cards, which XMage does not ask either."""
        if not p.stack:
            return
        if not self.cards.search_by_each_player(cast(str, p.stack[-1]["name"])):
            return
        following = self.actions[self.index + 1 : self.index + 2]
        if following and following[0].get("kind") == "ChooseUpTo":
            return
        if self.index + 1 >= len(self.trace):
            raise Unsupported("a search by each player who lost objects, at the end")
        after = self.trace[self.index + 1].creatures
        for seat, (before, now) in enumerate(zip(p.creatures, after)):
            if now < before:
                self.emit(f"addTarget({self.player(seat)}, TestPlayer.TARGET_SKIP);")

    def cast(self, action: Json, p: Position) -> None:
        who = self.player(action["player"])
        card = cast(str, action["card"])
        if action["face"]:
            raise Unsupported(f"casting {card}'s back face")
        costs = cast(int, action["costs"])
        if card in p.flashback:
            self.flashback(action, p)
            return
        name = card
        overload = self.cards.overload_bit(card)
        if overload is not None and costs >> overload & 1:
            if costs != 1 << overload:
                raise Unsupported(f"casting option {costs:#b} of {card}")
            name, costs = f"{card} with overload", 0
        self.prepare(who, p, starts=True)
        self.pay(who, p, None)
        spell = self.spell_on_top(p)
        targets = "TestPlayer.NO_TARGET"
        if self.cards.targets_in_cast(card):
            targets = java_string("^".join(self.take_targets(action["player"])))
        if p.stack and not any(cast(bool, item["spell"]) for item in p.stack):
            # XMage's spell abilities are named "Cast <card>"; an ability's text is its rule.
            self.emit(
                f"castSpell({self.at(p)}, {who}, {java_string(name)}, {targets}, "
                f'"Cast ", StackClause.WHILE_NOT_ON_STACK);'
            )
        elif spell is None and targets == "TestPlayer.NO_TARGET":
            self.emit(f"castSpell({self.at(p)}, {who}, {java_string(name)});")
        elif spell is None:
            self.emit(
                f"castSpell({self.at(p)}, {who}, {java_string(name)}, {targets});"
            )
        else:
            self.emit(
                f"castSpell({self.at(p)}, {who}, {java_string(name)}, {targets}, "
                f"{java_string(spell)});"
            )
        questions = self.cards.cast_answers(card)
        if costs >> len(questions):
            raise Unsupported(f"casting option {costs:#b} of {card}")
        for bit, (paid, unpaid) in enumerate(questions):
            answer = paid if costs >> bit & 1 else unpaid
            if answer:
                self.emit(f"setChoice({who}, {java_string(answer)});")
        if self.cards.unless_pays(card):
            self.unless_pays[card] = self.target_controller(action["player"], card)
        if self.cards.takes(card) is not None:
            self.take_pending = card

    def flashback(self, action: Json, p: Position) -> None:
        """Casts a card with flashback for its mana cost (Snapcaster Mage) from its owner's
        graveyard, which XMage activates as the card's "Flashback {cost}" ability. Its targets
        follow in the target queue. Onto a stack, it is unsupported: the ability that granted
        flashback resolved in the same Beryl action as what put it there, where XMage's
        commands resolve one object at a time."""
        who = self.player(action["player"])
        card = cast(str, action["card"])
        if action["costs"]:
            raise Unsupported(f"flashback of {card} with a casting option")
        if p.stack:
            raise Unsupported(f"flashback of {card} onto a stack")
        text = java_string(f"Flashback {self.faces.mana_cost[card]}")
        self.prepare(who, p, starts=True)
        self.pay(who, p, None)
        self.emit(f"activateAbility({self.at(p)}, {who}, {text});")

    def pay(self, who: str, p: Position, source: str | None) -> None:
        """Before a cast or activation at an empty stack, taps the mana sources Beryl's payment
        tapped, which are among the taps by the next position, less the activated `source`
        itself, by name and their one mana ability (`Cards.mana_text`). A payment with a source of
        several kinds of mana is left to XMage. One that sacrifices a source for mana of any
        color is unsupported: XMage asks the color, which the trace doesn't record."""
        taps = payment_taps(self.actions, [t.taps for t in self.trace], self.index)
        if taps is None:
            return
        if any(self.cards.sacrificed_for_mana_of_any_color(name) for name in taps):
            raise Unsupported(
                "a payment sacrificing a source for mana of a color XMage asks"
            )
        if p.stack:
            if self.payment_left_to_xmage and taps:
                # A spell cast onto a stack is paid by XMage too, from whatever an earlier
                # payment of XMage's choosing left untapped, which may be too little for it
                # (two spells needing {U} and {U}{U} from exactly three blue sources).
                raise Unsupported(
                    "a payment after one XMage chose the sources of, onto a stack"
                )
            # XMage chooses this payment's sources too, so a later payment that taps a source
            # by name may find it already tapped, depending on which ones XMage chose.
            self.payment_left_to_xmage = self.payment_left_to_xmage or bool(taps)
            return
        if source in taps:
            taps.remove(source)
        if any(self.cards.mana_choice(name) for name in taps):
            self.payment_left_to_xmage = True
            return
        for name in taps:
            text = self.cards.mana_text(name)
            if text is not None:
                if self.payment_left_to_xmage:
                    # XMage paid an earlier spell with sources of its own choosing, and may
                    # have tapped the one Beryl tapped here, so the command would fail or not
                    # depending on which it chose.
                    raise Unsupported(
                        "a payment tapping a source by name after one XMage chose the "
                        "sources of"
                    )
                self.emit(
                    f"tapSource({self.at(p)}, {who}, {java_string(name)}, "
                    f"{java_string(text)});"
                )

    def cast_targets(self) -> list[Json]:
        """The Target answers of the cast at hand: those that follow it, or else those Beryl
        gave without asking."""
        i = self.index
        answers: list[Json] = []
        for action in self.actions[i + 1 :]:
            if action["action"] != "Answer" or action["kind"] not in (
                "Target",
                "TargetCount",
            ):
                break
            answers.append(action)
        if not answers and i + 1 < len(self.trace):
            for forced in self.trace[i + 1].forced:
                if forced["kind"] != "Target":
                    break
                answers.append(forced)
        return answers

    def target_controller(self, seat: object, card: str) -> str:
        """The controller of the spell that `card`, cast at hand by `seat`, targets."""
        names = {
            cast(Json, a["pick"]).get("card")
            for a in self.cast_targets()
            if a["player"] == seat and a["kind"] == "Target"
        }
        for item in self.trace[self.index].stack:
            if item["name"] in names:
                return self.player(item["controller"])
        raise Unsupported(f"{card} without a target spell on the stack")

    def take_targets(self, seat: object) -> list[str]:
        """The cards the cast at hand targets (`cast_targets`); they are taken, not queued
        again."""
        names = []
        for answer in self.cast_targets():
            pick = cast(Json, answer["pick"])
            if answer["player"] != seat or (
                answer["kind"] == "Target" and "card" not in pick
            ):
                raise Unsupported("a target in the cast command that is not a card")
            if answer["kind"] == "Target":
                names.append(cast(str, pick["card"]))
            self.taken.add(id(answer))
        if not names:
            raise Unsupported("a cast command without its targets")
        return names

    def activate(self, action: Json, p: Position) -> None:
        who = self.player(action["player"])
        card = cast(str, action["card"])
        ability = cast(Json, action["ability"])
        ((how, index),) = ability.items()
        text = self.cards.ability_text(card, how, cast(int, index))
        if text is None:
            raise Unsupported(f"{card}'s ability {how} {index}")
        self.prepare(who, p, starts=True)
        self.pay(who, p, card)
        spell = self.spell_on_top(p)
        if spell is None:
            self.emit(f"activateAbility({self.at(p)}, {who}, {java_string(text)});")
        else:
            self.emit(
                f"activateAbility({self.at(p)}, {who}, {java_string(text)}, "
                f"TestPlayer.NO_TARGET, {java_string(spell)});"
            )

    def answer(self, action: Json, p: Position) -> None:
        if id(action) in self.taken:
            return
        who = self.player(action["player"])
        kind = cast(str, action["kind"])
        # A forced answer says its own; a scripted one answers the decision open at `p`.
        bending = action.get("purpose", p.purpose) == "waterbend"
        source = cast(str, p.stack[-1]["name"]) if p.stack else None
        if kind == "YesNo" and source is not None:
            self.unless_pays.pop(source, None)
        # "Include your tapped lands to untap?", XMage's shortcut for untapping them all,
        # asked only when its controller has no more tapped lands than it untaps: no leaves
        # the choice to Beryl's picks.
        untaps = source is not None and self.cards.untaps_up_to(source)
        if untaps and kind == "ChooseUpTo" and self.last_answer != (source, kind):
            self.emit(f'setOptionalChoice({who}, {self.at(p)}, "No");')
        self.last_answer = (source, kind)
        pick = cast(Json, action["pick"])
        what = cast(str, pick["pick"])
        card = cast("str | None", pick.get("card"))
        resolving = source or ""
        may_choose = self.cards.may_choose(resolving)
        if self.cards.takes(resolving) and kind == "ChooseObject" and card is not None:
            self.take(who, resolving, card)
        elif self.cards.scries(resolving) and kind == "ChooseUpTo":
            if self.scry is None:
                self.scry = (who, [], [])
            if card is not None:
                self.scry[1].append(card)
            else:
                bottom = "^".join(self.scry[1])
                skip = "TestPlayer.TARGET_SKIP"
                self.emit(
                    f"addTarget({who}, {java_string(bottom) if bottom else skip});"
                )
        elif self.scry is not None and kind == "ChooseObject" and card is not None:
            self.scry[2].append(card)
        elif (
            self.may_choose is not None and kind == "ChooseObject" and card is not None
        ):
            self.emit(f"{self.may_choose}({who}, {java_string(card)});")
            self.may_choose = None
        elif may_choose is not None and kind == "YesNo":
            asks, command = may_choose
            if asks:
                self.emit(f"setChoice({who}, {'true' if what == 'Yes' else 'false'});")
            if what == "Yes":
                self.may_choose = command
            elif not asks:
                skip = "CHOICE_SKIP" if command == "setChoice" else "TARGET_SKIP"
                self.emit(f"{command}({who}, TestPlayer.{skip});")
        elif self.cards.may_search(resolving) and kind == "YesNo":
            self.emit(f"setChoice({who}, {'true' if what == 'Yes' else 'false'});")
            if what == "Yes" and self.next_answer_kind() != "ChooseUpTo":
                self.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
        elif kind == "Priority" and what == "Pass":
            if p.stack:
                self.prepare(who, p)
                self.emit(f"resolveTop({self.at(p)}, {who});")
        elif kind == "Target" and card is not None:
            self.emit(f"addTarget({who}, {java_string(self.object_name(card, p))});")
        elif kind == "Target" and what == "Player":
            self.emit(f"addTarget({who}, {self.player(pick['player'])});")
        elif kind == "TargetCount" and what == "Number":
            if pick["n"] == 0:
                self.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
        elif kind == "Attacker" and card is not None:
            self.attack(who, card, p)
        elif kind in ("Attacker", "Blocker") and what == "Done":
            pass
        elif kind == "AttackTarget":
            # One that follows its attacker went with it (`attack`).
            raise Unsupported(
                "choosing what a creature put onto the battlefield attacks"
            )
        elif kind == "Blocker" and card is not None:
            attackers = self.attackers.get(p.turn, [])
            if len(attackers) != 1:
                raise Unsupported(f"a block among {len(attackers)} declared attackers")
            self.emit(
                f"block({self.turn(p.turn)}, {who}, {java_string(card)}, "
                f"{java_string(attackers[0])});"
            )
            # XMage asks a blocked trampler's division, which Beryl makes silently.
            amount = trample_amount(self.faces, attackers[0], card)
            if amount is not None:
                if p.turn in self.trampled:
                    raise Unsupported("a trampler blocked by more than one creature")
                self.trampled.add(p.turn)
                attacking = self.player(1 - cast(int, action["player"]))
                self.emit(f"setChoiceAmount({attacking}, {amount});")
        elif kind == "TriggerOrder" and card and self.cards.first_trigger_text(card):
            text = java_string(cast(str, self.cards.first_trigger_text(card)))
            self.emit(f"setChoice({who}, {text});")
        elif (kind in CHOICE_KINDS or bending) and card is not None:
            name = java_string(self.object_name(card, p))
            self.emit(f"{self.choice_command(source, kind)}({who}, {name});")
        elif (kind == "ChooseUpTo" or bending) and what == "Done":
            if self.choice_command(source, kind) == "addTarget":
                self.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
            else:
                self.emit(f"setChoice({who}, TestPlayer.CHOICE_SKIP);")
        elif kind in YES_NO_KINDS and what in ("Yes", "No", "Card"):
            self.emit(f"setChoice({who}, {'false' if what == 'No' else 'true'});")
        elif kind == "X" and what == "Number":
            self.emit(f'setChoice({who}, "X={pick["n"]}");')
        else:
            raise Unsupported(f"answering {kind} with {what}")

    def take(self, who: str, source: str, card: str) -> None:
        """One pick of a `Cards.takes` card: for the hand, queued as one target list once all are
        chosen, then for the bottom, from the choice queue."""
        if self.take_pending == source:
            self.take_pending = None
            self.taking = []
        taking = self.taking
        if taking is None:
            self.emit(f"setChoice({who}, {java_string(card)});")
            return
        taking.append(card)
        if len(taking) == self.cards.takes(source):
            self.emit(f"addTarget({who}, {java_string('^'.join(taking))});")
            self.taking = None

    def next_answer_kind(self) -> str | None:
        """The decision kind of the next action that is not an assertion, if it answers one."""
        for action in self.actions[self.index + 1 :]:
            if action["action"] != "Assert":
                return cast("str | None", action.get("kind"))
        return None

    def choice_command(self, source: str | None, kind: str) -> str:
        """The queue XMage takes a choice of a card from."""
        targets = self.cards.choices_as_targets(source or "") and kind in (
            "ChooseObject",
            "ChooseUpTo",
        )
        return "addTarget" if targets else "setChoice"

    def attack(self, who: str, card: str, p: Position) -> None:
        """Declares an attacker, at what the AttackTarget answer after it chooses, if any."""
        turn = self.turn(p.turn)
        self.attackers.setdefault(p.turn, []).append(card)
        target = None
        following = self.actions[self.index + 1 : self.index + 2]
        if following and following[0].get("kind") == "AttackTarget":
            target = cast(Json, following[0]["pick"])
            self.taken.add(id(following[0]))
        if target is None or target["pick"] == "DefendingPlayer":
            defender = "playerB" if who == "playerA" else "playerA"
            self.emit(f"attack({turn}, {who}, {java_string(card)}, {defender});")
        else:
            planeswalker = java_string(cast(str, target["card"]))
            self.emit(f"attack({turn}, {who}, {java_string(card)}, {planeswalker});")

    def check(self, assertion: Json, p: Position) -> None:
        what = cast(str, assertion["assert"])
        if what == "Decision":
            self.export.skipped.append("Decision: Beryl's decision surface")
            return
        if what == "Outcome":
            raise Unsupported("an outcome assertion")
        if p.decider is None:
            raise Unsupported(f"a {what} assertion after the game is over")
        if p.kind != "Priority":
            self.export.skipped.append(f"{what}: in the middle of a {p.kind} decision")
            return
        code = assertion_code(assertion, self.player(assertion.get("player")))
        who = self.player(p.decider)
        self.prepare(who, p)
        self.emit(
            f"runCode({java_string(what)}, {self.at(p)}, {who}, "
            f"(info, player, game) -> {code});"
        )

    def stop(self) -> None:
        """Stops at the first step after the last position that every turn has."""
        turn, step = self.last
        later = [s for s in EVERY_TURN if ORDER.index(s) > step]
        if later:
            self.emit(f"setStopAt({turn}, PhaseStep.{STEPS[later[0]]});")
        else:
            self.emit(f"setStopAt({turn + 1}, PhaseStep.UNTAP);")
        self.emit("execute();")


def export_one(entry: Json, index: int, faces: Faces, cards: set[str]) -> Export:
    export = Export(
        card=cast(str, entry["card"]),
        scenario=cast(str, entry["name"]),
        method=method_name(index, cast(str, entry["name"])),
    )
    if "failure" in entry:
        export.unsupported = f"Beryl fails it: {entry['failure']}"
        return export
    missing = sorted(set(setup_cards(entry)) - cards)
    if missing:
        export.unsupported = f"XMage lacks {', '.join(missing)}"
        return export
    try:
        Exporter(entry, faces, export).run()
    except Unsupported as e:
        export.unsupported = str(e)
        export.body.clear()
    return export


def setup_cards(entry: Json) -> Iterable[str]:
    for setup in cast(list[Json], cast(Json, entry["setup"])["players"]):
        for zone in ("hand", "graveyard", "exile", "library"):
            yield from cast(list[str], setup[zone])
        for permanent in cast(list[Json], setup["battlefield"]):
            yield cast(str, permanent["card"])


def export_all(dump: Sequence[Json], faces: Faces, cards: set[str]) -> list[Export]:
    counters: dict[str, int] = {}
    exports = []
    for entry in dump:
        card = cast(str, entry["card"])
        counters[card] = counters.get(card, 0) + 1
        exports.append(export_one(entry, counters[card], faces, cards))
    return exports
