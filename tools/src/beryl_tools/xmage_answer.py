"""Queuing scripted and forced answers for the XMage exporter (`xmage.py`)."""

from __future__ import annotations

from typing import TYPE_CHECKING, cast

from .card_facts import Json, Unsupported
from .xmage_java import (
    ANOTHER_CARD,
    CHOICE_KINDS,
    COLORS,
    YES_NO_KINDS,
    creature_type,
    java_string,
    offered_cast_choices,
)
from .xmage_trace import Position

if TYPE_CHECKING:
    from .xmage import Exporter


def queue_answer(
    exporter: Exporter,
    action: Json,
    p: Position,
    of: str | None = None,
    *,
    enter_as_copy: bool = False,
) -> None:
    """Queues one answer's commands. `of` names what it answers where the stack at `p` no
    longer holds it, as for a forced answer of a spell that has since resolved."""
    if id(action) in exporter.taken:
        return
    who = exporter.player(action["player"])
    kind = cast(str, action["kind"])
    pick = cast(Json, action["pick"])
    what = cast(str, pick["pick"])
    card = cast("str | None", pick.get("card"))
    if enter_as_copy:
        # XMage asks whether to copy, then which permanent.
        exporter.emit(f"setChoice({who}, {'false' if card is None else 'true'});")
        if card is not None:
            exporter.emit(f"setChoice({who}, {java_string(p.object_name(card))});")
        return
    # A forced answer says its own; a scripted one answers the decision open at `p`.
    bending = action.get("purpose", p.purpose) == "waterbend"
    if action.get("purpose", p.purpose) == "pile-order":
        # XMage asks the order of a melded permanent's two cards, the first chosen lowest
        # as in Beryl (`ZonesHandler.chooseOrder`), and moves a merged permanent's cards
        # without asking.
        if card is not None and exporter.cards.shapes.get(card) == "Meld":
            exporter.emit(f"setChoice({who}, {java_string(p.object_name(card))});")
        return
    source = of or (cast(str, p.stack[-1]["name"]) if p.stack else None)
    if kind == "PaymentPlan" and exporter.payments.as_made:
        # The payment's taps, replayed with the mana each made, chose the plan already.
        return
    if kind == "ChooseUpTo" and what == "Cast":
        # XMage asks separately whether to cast and which card to cast before the find.
        raise Unsupported("casting while searching a library")
    if kind == "CastOffer" and what == "Cast":
        exporter.payments.float_for_offer(
            who, exporter.at(p), p, exporter.payment(), exporter.lines
        )
        exporter.lines.extend(offered_cast_choices(exporter.cards, who, pick))
        return
    if kind == "YesNo" and source is not None:
        exporter.unless_pays.pop(source, None)
    # "Include your tapped lands to untap?", XMage's shortcut for untapping them all,
    # asked only when its controller has no more tapped lands than it untaps: no leaves
    # the choice to Beryl's picks.
    untaps = source is not None and exporter.cards.untaps_up_to(source)
    if untaps and kind == "ChooseUpTo":
        # XMage only offers tapped lands. A payment either player delegated may have
        # left a land untapped that Beryl chose here.
        if card is not None and exporter.payments.left_to_xmage:
            raise Unsupported("an untap choice after XMage chose a payment's sources")
        if exporter.last_answer != (source, kind):
            exporter.emit(f'setOptionalChoice({who}, {exporter.at(p)}, "No");')
    exporter.last_answer = (source, kind)
    if kind == "ChooseUpTo" and exporter.cards.hidden_pick(source):
        # XMage asks each player whether to put a card, then which, from the hand.
        if card is None:
            exporter.emit(f"setChoice({who}, false);")
        else:
            exporter.emit(f"setChoice({who}, true);")
            exporter.emit(f"addTarget({who}, {java_string(p.object_name(card))});")
        return
    resolving = source or ""
    if kind == "ChooseUpTo" and exporter.cards.puts_any_on_bottom(resolving):
        # XMage's `TargetCardInHand` of any number takes the whole list at once from the
        # choice queue, so only the pick of none is queued, as a skip.
        if card is not None:
            raise Unsupported(
                "choosing hand cards for the library bottom one at a time"
            )
        exporter.emit(f"setChoice({who}, TestPlayer.CHOICE_SKIP);")
        return
    may_choose = exporter.cards.may_choose(resolving)
    if exporter.cards.takes(resolving) and kind == "ChooseObject" and card is not None:
        take(exporter, who, resolving, card)
    elif exporter.cards.scries(resolving) and kind == "ChooseUpTo":
        if exporter.scry is None:
            exporter.scry = (who, [], [])
        if card is not None:
            exporter.scry[1].append(card)
        else:
            exporter.queue_scry_bottom()
    elif exporter.scry is not None and kind == "ChooseObject" and card is not None:
        exporter.scry[2].append(card)
    elif (
        exporter.may_choose is not None and kind == "ChooseObject" and card is not None
    ):
        exporter.emit(f"{exporter.may_choose}({who}, {java_string(card)});")
        exporter.may_choose = None
    elif may_choose is not None and kind == "YesNo":
        asks, command = may_choose
        if asks:
            exporter.emit(f"setChoice({who}, {'true' if what == 'Yes' else 'false'});")
        if what == "Yes":
            exporter.may_choose = command
        elif not asks:
            skip = "CHOICE_SKIP" if command == "setChoice" else "TARGET_SKIP"
            exporter.emit(f"{command}({who}, TestPlayer.{skip});")
    elif exporter.cards.may_search(resolving) and kind == "YesNo":
        exporter.emit(f"setChoice({who}, {'true' if what == 'Yes' else 'false'});")
        if what == "Yes" and next_answer_kind(exporter) != "ChooseUpTo":
            exporter.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
    elif kind == "Target" and p.kept is not None and "purpose" not in action:
        exporter.retargeted = who
        for line in p.new_target(who, pick, exporter.player):
            exporter.emit(line)
    elif kind == "Priority" and what == "Pass":
        if p.stack:
            exporter.prepare(who, p)
            exporter.emit(f"resolveTop({exporter.at(p)}, {who});")
    elif kind == "Target" and card is not None:
        exporter.emit(f"addTarget({who}, {java_string(p.object_name(card))});")
        # XMage asks for "up to N" targets until N are chosen or it is told to stop.
        if exporter.targets_short is not None:
            exporter.targets_short -= 1
            if "purpose" in action:
                # Beryl answered silently because the target was the only one left, so
                # XMage has no candidate to ask about and queues no skip.
                exporter.targets_short = None
            elif exporter.targets_short == 0:
                exporter.targets_short = None
                exporter.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
    elif kind == "Target" and what == "Player":
        exporter.emit(f"addTarget({who}, {exporter.player(pick['player'])});")
    elif kind == "TargetCount" and what == "Number":
        n = cast(int, pick["n"])
        if n == 0:
            exporter.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
        elif source is not None and n < exporter.most_targets(source):
            exporter.targets_short = n
    elif kind in (
        "Attacker",
        "AttackTarget",
        "Blocker",
        "BlockTarget",
        "DamageDivision",
    ):
        exporter.combat.answer(exporter, action, p)
    elif kind == "TriggerOrder" and card and exporter.cards.first_trigger_text(card):
        text = java_string(cast(str, exporter.cards.first_trigger_text(card)))
        exporter.emit(f"setChoice({who}, {text});")
    elif action.get("purpose", p.purpose) == "card-name":
        # XMage asks for a name (`ChooseACardNameEffect`); Beryl numbers the game's names.
        exporter.emit(f"setChoice({who}, {java_string(card or ANOTHER_CARD)});")
    elif action.get("purpose", p.purpose) == "color" and what == "Number":
        # XMage names the color (`ChoiceColor`); Beryl numbers it as CR 105.1 does.
        exporter.emit(f"setChoice({who}, {java_string(COLORS[cast(int, pick['n'])])});")
    elif action.get("purpose", p.purpose) == "creature-type" and what == "Named":
        exporter.emit(f"setChoice({who}, {java_string(creature_type(pick['name']))});")
    elif action.get("purpose", p.purpose) == "mutate" and what in ("Yes", "No"):
        # XMage asks "UNDER or OVER" (`PermanentImpl.mutate`), yes for under; Beryl's yes
        # is over.
        exporter.emit(f"setChoice({who}, {'true' if what == 'No' else 'false'});")
    elif action.get("purpose", p.purpose) == "wish":
        exporter.emit(f"setChoice({who}, {'false' if card is None else 'true'});")
        if card is not None:
            exporter.emit(f"setChoice({who}, {java_string(card)});")
    elif action.get("purpose", p.purpose) == "replace":
        # Dredge's yes or no (`chooseUse`); mandatory effects' order by source name.
        yes = "false" if card is None else "true"
        name = java_string(p.object_name(card)) if card else yes
        exporter.emit(f"setChoice({who}, {name if kind == 'ChooseObject' else yes});")
    elif (kind in CHOICE_KINDS or bending) and card is not None:
        name = java_string(p.object_name(card))
        purpose = action.get("purpose", p.purpose)
        text = exporter.faces.oracle.get(resolving, "").lower()
        # XMage's simultaneous discard effect uses TargetDiscard for each player.
        discard_together = purpose == "discard" and (
            "each opponent discards" in text or "each player discards" in text
        )
        command = (
            "addTarget" if discard_together else choice_command(exporter, source, kind)
        )
        exporter.emit(f"{command}({who}, {name});")
    elif action.get("purpose", p.purpose) == "delve" and what == "Done":
        pass  # XMage's delve ends once it has as many cards as generic mana left to pay
    elif (kind == "ChooseUpTo" or bending) and what == "Done":
        if choice_command(exporter, source, kind) == "addTarget":
            exporter.emit(f"addTarget({who}, TestPlayer.TARGET_SKIP);")
        else:
            exporter.emit(f"setChoice({who}, TestPlayer.CHOICE_SKIP);")
    elif kind == "CastOffer" and what == "Pass":
        exporter.emit(f"setChoice({who}, false);")
    elif kind in YES_NO_KINDS and what in ("Yes", "No", "Card"):
        exporter.emit(f"setChoice({who}, {'false' if what == 'No' else 'true'});")
    elif kind == "X" and what == "Number":
        exporter.emit(f'setChoice({who}, "X={pick["n"]}");')
    else:
        raise Unsupported(f"answering {kind} with {what}")


def take(exporter: Exporter, who: str, source: str, card: str) -> None:
    """One pick of a `Cards.takes` card: for the hand, queued as one target list once all are
    chosen, then for the bottom, from the choice queue."""
    if exporter.take_pending == source:
        exporter.take_pending = None
        exporter.taking = []
    taking = exporter.taking
    if taking is None:
        exporter.emit(f"setChoice({who}, {java_string(card)});")
        return
    taking.append(card)
    if len(taking) == exporter.cards.takes(source):
        exporter.emit(f"addTarget({who}, {java_string('^'.join(taking))});")
        exporter.taking = None


def next_answer_kind(exporter: Exporter) -> str | None:
    """The decision kind of the next action that is not an assertion, if it answers one."""
    for action in exporter.actions[exporter.index + 1 :]:
        if action["action"] != "Assert":
            return cast("str | None", action.get("kind"))
    return None


def choice_command(exporter: Exporter, source: str | None, kind: str) -> str:
    """The queue XMage takes a choice of a card from."""
    targets = exporter.cards.choices_as_targets(source or "") and kind in (
        "ChooseObject",
        "ChooseUpTo",
    )
    return "addTarget" if targets else "setChoice"
