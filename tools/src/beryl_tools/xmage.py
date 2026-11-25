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
- Answers, including forced ones, are queued in order. The trace identifies entry copy
  choices directly. Card facts translate other questions that XMage asks differently.
- Coin flips replay Beryl's results, and payments replay the sources Beryl tapped.
- Assertions run inside `runCode` at their position, through XMage's `assert*` family where it
  states the same fact and `BerylScenarioTest`'s helpers otherwise. The exporter skips Beryl's
  decision assertions, assertions between priority points and Tapped assertions after a payment
  whose sources XMage chose.
"""

from __future__ import annotations

from typing import cast

from . import card_facts
from .card_facts import Cards, Json, Unsupported, as_their_own, payment_position
from .xmage_payment import Payments
from .xmage_trace import (
    Combat,
    FORCED_ASKED,
    FORCED_CHOICES,
    FORCED_COSTS,
    FORCED_TARGETS,
    Position,
    cast_targets,
    resolution_skips,
)
from .xmage_java import (
    ANOTHER_CARD,
    creature_type,
    battlefield_card,
    CHOICE_KINDS,
    COLORS,
    YES_NO_KINDS,
    EVERY_TURN,
    ORDER,
    STEPS,
    COUNTERS,
    Export,
    Faces,
    activate_on_stack,
    assertion_code,
    cast_choices,
    divided_damage,
    java_string,
    offered_cast_choices,
    spell_cast,
    outcome_lines,
    sideboard_setup_cards,
    turn_face_up_rule,
)


class Exporter:
    """Translates one scenario into the statements of a test method."""

    def __init__(self, entry: Json, faces: Faces, export: Export) -> None:
        self.entry = entry
        self.faces = faces
        self.export = export
        self.cards = Cards(cast(Json, entry.get("cards", {})), faces.oracle)
        self.cards.track_merges(cast(list[Json], entry["actions"]))
        setup = cast(Json, entry["setup"])
        self.setup: Json = setup
        self.first_turn: int = cast(int, setup["turn"])
        self.restore_first_draw: bool = (
            self.first_turn > 1 and setup["step"] == "Upkeep"
        )
        self.active: int = cast(int, setup["active"])
        self.lines: list[str] = export.body
        self.combat = Combat(faces)
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
        # Each player's payments, which XMage may pay with sources of its own choosing.
        self.payments = Payments(self.cards)
        # How many targets of an "up to N" spec are still to come when fewer than N were
        # announced: XMage asks for more until told to stop.
        self.targets_short: int | None = None
        # After retargeting a copy, queue asserts and Resolves for the player who retargeted it.
        # The other player may have held priority before the copy was made.
        self.retargeted: str | None = None
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
        # What the test checks after the game has run, for an outcome assertion.
        self.ending: list[str] = []

    def player(self, seat: object) -> str:
        return "playerA" if seat == self.active else "playerB"

    def most_targets(self, card: str) -> int:
        """The most targets the first targeted ability of `card` may choose."""
        for ability in self.cards.abilities(card):
            targets = cast(list[Json], ability.get("targets", []))
            if targets:
                return cast(int, targets[0]["count"])
        return 0

    def turn(self, turn: int) -> int:
        return turn - self.first_turn + 1

    def at(self, p: Position) -> str:
        """`turn, PhaseStep.STEP` for a position."""
        turn = self.turn(p.turn)
        if turn == 1 and STEPS[p.step] == "DRAW" and not self.restore_first_draw:
            raise Unsupported("the draw step of XMage's first turn, which it skips")
        return f"{turn}, PhaseStep.{STEPS[p.step]}"

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

    def run(self) -> None:
        self.arrange()
        raw = cast(list[Json], self.entry["trace"])
        trace = [Position.of(t) for t in raw]
        if (
            trace
            and trace[0].stack
            and trace[0].step == self.setup["step"] == "PrecombatMain"
        ):
            # XMage starts at the beginning of the turn and processes setup triggers
            # before the first main phase, where Beryl's script expects them to resolve.
            raise Unsupported("a setup trigger pending at the first main phase")
        self.trace = trace
        self.actions = actions = [
            as_their_own(a, raw[i].get("decides_for")) if i < len(raw) else a
            for i, a in enumerate(self.actions)
        ]
        for i, (action, p) in enumerate(zip(actions, trace)):
            self.index = i
            if p.kind == "Priority":
                self.payments.resolve_at = None
            self.unasked(p)
            for answer in p.forced:
                self.forced(answer, p)
            for flip in p.flips:
                won = "true" if flip["won"] else "false"
                self.emit(f"setFlipCoinResult({self.player(flip['player'])}, {won});")
            self.reach(p.turn, p.step)
            if action["action"] != "Answer":
                self.end_scry()
            p.check_simultaneous_emblem_triggers()
            self.act(action, p)
            kept = action["action"] == "Answer" and p.kept is not None
            if not p.stack or not (kept or action["action"] in ("Assert", "Resolve")):
                self.retargeted = None
        self.end_scry()
        if self.take_pending is not None:
            raise Unsupported(f"{self.take_pending} putting cards into a hand unasked")
        self.stop()

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
        if answer.get("enter_as_copy"):
            self.answer(answer, p, enter_as_copy=True)
            return
        kind = answer["kind"]
        scrying = self.scry is not None and kind == "ChooseObject"
        asked = kind in FORCED_ASKED or answer.get("purpose") in FORCED_COSTS
        # Each player's hidden pick, which XMage asks of every player as Beryl does. A forced
        # answer belongs to the position after it, where the spell may have resolved and what
        # it put onto the battlefield triggered: then it answers the spell the last answer did.
        top = cast(str, p.stack[-1]["name"]) if p.stack else None
        hiding = [c for c in (top, self.last_answer[0]) if self.cards.hidden_pick(c)]
        if kind == "ChooseUpTo" and hiding:
            self.answer(answer, p, hiding[0])
            return
        if asked or self.may_choose is not None or scrying:
            self.answer(answer, p)
            return
        pick = cast(Json, answer["pick"])
        if kind == "CastOffer" and pick.get("pick") == "Pass":
            # Beryl offers only the casts that are legal and passes for a player with none.
            # XMage asks whether to cast a card or its copy before it checks, as Arcane
            # Proxy's "Cast copy of Counterspell without paying its mana cost?" with no spell
            # to counter, so it may ask a question to answer no.
            who = self.player(answer["player"])
            turn = self.turn(cast(int, answer["turn"]))
            step = STEPS[cast(str, answer["step"])]
            self.emit(f'setOptionalChoice({who}, {turn}, PhaseStep.{step}, "No");')
            return
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
        # Beryl ends a scry unasked once every card is picked for the bottom, so no Done answer
        # follows to queue XMage's one pick of the bottom cards.
        if self.scry is not None and self.scry[1]:
            self.queue_scry_bottom()
        self.scry = None

    def queue_scry_bottom(self) -> None:
        """Queues XMage's one answer to the scry's bottom pick: the cards picked, or a skip for
        none, and forgets them so that a scry is answered once."""
        assert self.scry is not None
        who, picked, _ = self.scry
        # XMage also asks the order of two or more cards for the bottom, which Beryl sets unasked.
        if len(picked) > 1:
            raise Unsupported("ordering a scry's cards on the bottom")
        bottom = "^".join(picked)
        skip = "TestPlayer.TARGET_SKIP"
        self.emit(f"addTarget({who}, {java_string(bottom) if bottom else skip});")
        picked.clear()

    def arrange(self) -> None:
        self.emit("removeAllCardsFromLibrary(playerA);")
        self.emit("removeAllCardsFromLibrary(playerB);")
        self.emit("skipInitShuffling();")
        # XMage's shared random source starts unseeded, so a shuffle during the scenario (such
        # as Timetwister's) drew different cards on each run. Seeding it with the scenario's seed
        # makes every run play the same game, as Forge's harness already does.
        self.emit(f"mage.util.RandomUtil.setSeed({cast(int, self.setup['seed'])}L);")
        start = cast(str, self.setup["step"])
        # XMage runs the opening main phase's turn-based action before giving priority.
        if ORDER.index(start) >= ORDER.index("PrecombatMain"):
            self.emit("skipOpeningMainLore();")
        # XMage's duel skips its first draw (CR 103.8a). A later Beryl upkeep still has a draw.
        if self.restore_first_draw:
            self.emit(
                'runCode("restore the draw step", 1, PhaseStep.UPKEEP, playerA, '
                "(info, player, game) -> game.getState().getTurnMods()"
                ".useNextSkipStep(player.getId(), PhaseStep.DRAW));"
            )
        self.cards.refuse_identical_damage_replacements(self.setup)
        for seat, setup in enumerate(cast(list[Json], self.setup["players"])):
            who = self.player(seat)
            life = cast(int, setup["life"])
            if life != 20:
                self.emit(f"setLife({who}, {life});")
            for key in ("hand", "battlefield", "graveyard", "exile", "library"):
                if key == "battlefield":
                    for index, permanent in enumerate(cast(list[Json], setup[key])):
                        self.permanent(who, permanent, start, index)
                    continue
                zone = "EXILED" if key == "exile" else key.upper()
                names = cast(list[str], setup[key])
                # XMage adds these cards to the top, so enter them bottom first.
                for name in (
                    reversed(names) if key in ("graveyard", "library") else names
                ):
                    printed = java_string(self.cards.printed_name(name))
                    self.emit(f"addCard(Zone.{zone}, {who}, {printed});")
            self.lines.extend(sideboard_setup_cards(who, setup, self.cards))
            # XMage plays its turn 1 from the untap step, where Beryl's setup starts at its step
            # with the earlier ones done: a permanent that flips a coin at the upkeep flips one
            # XMage would lose half the time. Beryl's setup skipped that flip, so XMage wins it.
            flips = any(
                self.cards.flips_a_coin_each_turn(cast(str, p["card"]))
                for p in cast(list[Json], setup["battlefield"])
            )
            if flips and who == "playerA" and start not in ("Untap", "Upkeep"):
                self.emit(f"setFlipCoinResult({who}, true);")
        self.emit("setStrictChooseMode(true);")

    def permanent(self, who: str, permanent: Json, start: str, index: int) -> None:
        self.lines.extend(
            battlefield_card(self.faces, self.cards, who, permanent, start, index)
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
            who = self.retargeted or self.player(p.decider)
            self.prepare(who, p)
            self.payments.resolve_at = (p.turn, p.step, len(self.lines))
            self.emit(f"resolveTop({self.at(p)}, {who});")
            self.lines.extend(
                resolution_skips(
                    self.cards,
                    p,
                    self.trace[self.index + 1 : self.index + 2],
                    self.actions[self.index + 1 : self.index + 2],
                    self.at(p),
                    self.player,
                )
            )
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
            self.answer(action, p, enter_as_copy=p.enter_as_copy)
        elif kind == "TurnFaceUp":
            self.turn_face_up(action, p)
        else:
            raise Unsupported(f"action {kind}")

    def turn_face_up(self, action: Json, p: Position) -> None:
        who = self.player(action["player"])
        card = cast(str, action["card"])
        self.prepare(who, p)
        self.pay(who, p, None)
        text = turn_face_up_rule(
            self.cards, card, self.faces.mana_cost[card], card in p.morphed
        )
        self.emit(activate_on_stack(self.at(p), who, text, p.topmost_spell()))

    def cast(self, action: Json, p: Position) -> None:
        who = self.player(action["player"])
        card = cast(str, action["card"])
        face = cast(int, action["face"])
        chosen = spell_cast(self.cards, card, face, p.supported_castings(action))
        name = chosen.command
        if Position.delves_next(self.trace, self.index):
            payment = self.payment()
            paid = None if payment is None else list(payment.taps)
            self.cards.delve_reachable(self.setup, action["player"], paid)
        if face == 0 and name == card and card in p.flashback:
            self.flashback(action, p)
            return
        self.prepare(who, p, starts=True)
        self.pay(who, p, None)
        # XMage's stack clause asks for a spell anywhere on the stack, so a cast in response to
        # an ability above a spell (a storm trigger above its spell) waits for that spell too.
        spell = p.topmost_spell()
        targets = "TestPlayer.NO_TARGET"
        if self.cards.targets_in_cast(card, chosen.face):
            targets = java_string("^".join(self.take_targets(action["player"])))
        elif self.cards.targets_abilities(card, chosen.face):
            names = self.take_targets(action["player"])
            targets = java_string("^".join(p.stack_ability(n) for n in names))
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
        cost_face = face if chosen.face is None else chosen.face
        damage = self.cards.instruction(card, "DealDamage", cost_face)
        if damage and "divided" in damage:
            answers = cast_targets(self.actions, self.trace, self.index)
            self.lines.extend(
                divided_damage(
                    who,
                    answers,
                    cast(int, damage["divided"]),
                    p.object_name,
                    self.player,
                )
            )
            self.taken.update(map(id, answers))
        self.lines.extend(cast_choices(self.cards, who, card, chosen.costs, cost_face))
        if self.cards.unless_pays(card, chosen.face):
            self.unless_pays[chosen.name] = self.target_controller(
                action["player"], card
            )
        if self.cards.takes(card, chosen.face) is not None:
            self.take_pending = chosen.name

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
        """Replay the trace's payment for the action at hand (`Payments.pay`), excluding
        `source`, an activated ability's own tap."""
        position = self.payment()
        if position is None:
            self.payments.as_made = False
            return
        taps, mana = list(position.taps), list(position.mana)
        self.lines.extend(
            self.payments.pay(who, self.at(p), bool(p.stack), taps, mana, source)
        )

    def payment(self) -> Position | None:
        at = payment_position(self.actions, [t.taps for t in self.trace], self.index)
        return self.trace[at] if at is not None else None

    def target_controller(self, seat: object, card: str) -> str:
        """The controller of the spell that `card`, cast at hand by `seat`, targets."""
        names = {
            cast(Json, a["pick"]).get("card")
            for a in cast_targets(self.actions, self.trace, self.index)
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
        for answer in cast_targets(self.actions, self.trace, self.index):
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
        written, how, index = self.cards.ability_card(card, action["ability"])
        if mana := self.cards.instant_mana_text(written, how, index):
            self.prepare(who, p, starts=True)  # off the stack (CR 605.3b)
            return self.emit(self.cards.tap_source(self.at(p), who, card, mana))
        text = self.cards.ability_text(written, how, index)
        if text is None:
            raise Unsupported(f"{card}'s ability {how} {index}")
        self.prepare(who, p, starts=True)
        self.pay(who, p, card)
        self.emit(activate_on_stack(self.at(p), who, text, p.spell_on_top()))

    def answer(
        self,
        action: Json,
        p: Position,
        of: str | None = None,
        *,
        enter_as_copy: bool = False,
    ) -> None:
        """Queues one answer's commands. `of` names what it answers where the stack at `p` no
        longer holds it, as for a forced answer of a spell that has since resolved."""
        if id(action) in self.taken:
            return
        who = self.player(action["player"])
        kind = cast(str, action["kind"])
        pick = cast(Json, action["pick"])
        what = cast(str, pick["pick"])
        card = cast("str | None", pick.get("card"))
        if enter_as_copy:
            # XMage asks whether to copy, then which permanent.
            self.emit(f"setChoice({who}, {'false' if card is None else 'true'});")
            if card is not None:
                self.emit(f"setChoice({who}, {java_string(p.object_name(card))});")
            return
        # A forced answer says its own; a scripted one answers the decision open at `p`.
        bending = action.get("purpose", p.purpose) == "waterbend"
        source = of or (cast(str, p.stack[-1]["name"]) if p.stack else None)
        if kind == "PaymentPlan" and self.payments.as_made:
            # The payment's taps, replayed with the mana each made, chose the plan already.
            return
        if kind == "ChooseUpTo" and what == "Cast":
            # XMage asks separately whether to cast and which card to cast before the find.
            raise Unsupported("casting while searching a library")
        if kind == "CastOffer" and what == "Cast":
            self.payments.float_for_offer(
                who, self.at(p), p, self.payment(), self.lines
            )
            self.lines.extend(offered_cast_choices(self.cards, who, pick))
            return
        if kind == "YesNo" and source is not None:
            self.unless_pays.pop(source, None)
        # "Include your tapped lands to untap?", XMage's shortcut for untapping them all,
        # asked only when its controller has no more tapped lands than it untaps: no leaves
        # the choice to Beryl's picks.
        untaps = source is not None and self.cards.untaps_up_to(source)
        if untaps and kind == "ChooseUpTo":
            # XMage only offers tapped lands. A payment either player delegated may have
            # left a land untapped that Beryl chose here.
            if card is not None and self.payments.left_to_xmage:
                raise Unsupported(
                    "an untap choice after XMage chose a payment's sources"
                )
            if self.last_answer != (source, kind):
                self.emit(f'setOptionalChoice({who}, {self.at(p)}, "No");')
        self.last_answer = (source, kind)
        if kind == "ChooseUpTo" and self.cards.hidden_pick(source):
            # XMage asks each player whether to put a card, then which, from the hand.
            if card is None:
                self.emit(f"setChoice({who}, false);")
            else:
                self.emit(f"setChoice({who}, true);")
                self.emit(f"addTarget({who}, {java_string(p.object_name(card))});")
            return
        resolving = source or ""
        if kind == "ChooseUpTo" and self.cards.puts_any_on_bottom(resolving):
            # XMage's `TargetCardInHand` of any number takes the whole list at once from the
            # choice queue, so only the pick of none is queued, as a skip.
            if card is not None:
                raise Unsupported(
                    "choosing hand cards for the library bottom one at a time"
                )
            self.emit(f"setChoice({who}, TestPlayer.CHOICE_SKIP);")
            return
        may_choose = self.cards.may_choose(resolving)
        if self.cards.takes(resolving) and kind == "ChooseObject" and card is not None:
            self.take(who, resolving, card)
        elif self.cards.scries(resolving) and kind == "ChooseUpTo":
            if self.scry is None:
                self.scry = (who, [], [])
            if card is not None:
                self.scry[1].append(card)
            else:
                self.queue_scry_bottom()
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
        elif kind == "Target" and p.kept is not None and "purpose" not in action:
            self.retargeted = who
            for line in p.new_target(who, pick, self.player):
                self.emit(line)
        elif kind == "Priority" and what == "Pass":
            if p.stack:
                self.prepare(who, p)
                self.emit(f"resolveTop({self.at(p)}, {who});")
        elif kind == "Target" and card is not None:
            self.emit(f"addTarget({who}, {java_string(p.object_name(card))});")
            # XMage asks for "up to N" targets until N are chosen or it is told to stop.
            if self.targets_short is not None:
                self.targets_short -= 1
                if self.targets_short == 0:
                    self.targets_short = None
                    self.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
        elif kind == "Target" and what == "Player":
            self.emit(f"addTarget({who}, {self.player(pick['player'])});")
        elif kind == "TargetCount" and what == "Number":
            n = cast(int, pick["n"])
            if n == 0:
                self.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
            elif source is not None and n < self.most_targets(source):
                self.targets_short = n
        elif kind in (
            "Attacker",
            "AttackTarget",
            "Blocker",
            "BlockTarget",
            "DamageDivision",
        ):
            self.combat.answer(self, action, p)
        elif kind == "TriggerOrder" and card and self.cards.first_trigger_text(card):
            text = java_string(cast(str, self.cards.first_trigger_text(card)))
            self.emit(f"setChoice({who}, {text});")
        elif action.get("purpose", p.purpose) == "card-name":
            # XMage asks for a name (`ChooseACardNameEffect`); Beryl numbers the game's names.
            self.emit(f"setChoice({who}, {java_string(card or ANOTHER_CARD)});")
        elif action.get("purpose", p.purpose) == "color" and what == "Number":
            # XMage names the color (`ChoiceColor`); Beryl numbers it as CR 105.1 does.
            self.emit(f"setChoice({who}, {java_string(COLORS[cast(int, pick['n'])])});")
        elif action.get("purpose", p.purpose) == "creature-type" and what == "Named":
            self.emit(f"setChoice({who}, {java_string(creature_type(pick['name']))});")
        elif action.get("purpose", p.purpose) == "mutate" and what in ("Yes", "No"):
            # XMage asks "UNDER or OVER" (`PermanentImpl.mutate`), yes for under; Beryl's yes
            # is over.
            self.emit(f"setChoice({who}, {'true' if what == 'No' else 'false'});")
        elif action.get("purpose", p.purpose) == "wish":
            self.emit(f"setChoice({who}, {'false' if card is None else 'true'});")
            if card is not None:
                self.emit(f"setChoice({who}, {java_string(card)});")
        elif action.get("purpose", p.purpose) == "replace":
            # Dredge's yes or no (`chooseUse`); mandatory effects' order by source name.
            yes = "false" if card is None else "true"
            name = java_string(p.object_name(card)) if card else yes
            self.emit(f"setChoice({who}, {name if kind == 'ChooseObject' else yes});")
        elif (kind in CHOICE_KINDS or bending) and card is not None:
            name = java_string(p.object_name(card))
            purpose = action.get("purpose", p.purpose)
            text = self.faces.oracle.get(resolving, "").lower()
            # XMage's simultaneous discard effect uses TargetDiscard for each player.
            discard_together = purpose == "discard" and (
                "each opponent discards" in text or "each player discards" in text
            )
            command = (
                "addTarget" if discard_together else self.choice_command(source, kind)
            )
            self.emit(f"{command}({who}, {name});")
        elif action.get("purpose", p.purpose) == "delve" and what == "Done":
            pass  # XMage's delve ends once it has as many cards as generic mana left to pay
        elif (kind == "ChooseUpTo" or bending) and what == "Done":
            if self.choice_command(source, kind) == "addTarget":
                self.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
            else:
                self.emit(f"setChoice({who}, TestPlayer.CHOICE_SKIP);")
        elif kind == "CastOffer" and what == "Pass":
            self.emit(f"setChoice({who}, false);")
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

    def check(self, assertion: Json, p: Position) -> None:
        what = cast(str, assertion["assert"])
        if what == "Decision":
            self.export.skipped.append("Decision: Beryl's decision surface")
            return
        if what == "Outcome":
            self.ending = outcome_lines(assertion, self.player)
            return
        if p.decider is None:
            raise Unsupported(f"a {what} assertion after the game is over")
        if p.kind != "Priority":
            self.export.skipped.append(f"{what}: in the middle of a {p.kind} decision")
            return
        if what in ("Tapped", "ManaPool") and self.payments.left_to_xmage:
            # XMage paid an earlier cost from sources of its own choosing, which may not be
            # the ones Beryl tapped (Urborg and a Plains where XMage taps a Swamp and a Plains),
            # so which permanents are tapped and how much they floated can differ.
            self.export.skipped.append(f"{what}: after XMage chose a payment's sources")
            return
        code = assertion_code(assertion, self.player, self.cards)
        who = self.retargeted or self.player(p.decider)
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
        self.lines.extend(self.ending)
