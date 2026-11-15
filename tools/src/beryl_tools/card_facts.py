"""What the oracle exporters (`xmage.py`, `forge.py`) need to know of a scenario's cards, read
from the description `beryl_scenario dump` gives of each card a scenario names (each entry's
`cards`): its faces' abilities with their costs, the mana they make, casting options and what
resolving them does.

Where Beryl's questions and an oracle's differ, the difference follows from what a card does, not
from which card it is: a loyalty ability's text in XMage starts with its loyalty change, a land
with one mana ability taps by that ability's text, a spell that scries asks the cards for the
bottom as one target list. Each rule below says what difference it encodes. No rule names a
card.
"""

from __future__ import annotations

import json
import re
from collections.abc import Iterable, Sequence
from typing import cast

Json = dict[str, object]

# Beryl's colors of mana as XMage's mana symbols.
SYMBOLS: dict[str, str] = {
    "White": "{W}",
    "Blue": "{U}",
    "Black": "{B}",
    "Red": "{R}",
    "Green": "{G}",
    "Colorless": "{C}",
}

# Counter kinds as their name in a cost's text, where it is not the kind in lower case.
COUNTER_WORDS: dict[str, str] = {"PlusOne": "+1/+1", "MinusOne": "-1/-1"}

# The instructions whose choices of cards XMage takes from the target queue, where Beryl asks
# them as choices: a library search, and the cards put back on a library.
CHOICES_AS_TARGETS = ("Search", "PutBack", "Fateseal")

# How many words of an ability's effect follow its cost in the text that names it: enough to
# tell two abilities of one cost apart.
EFFECT_WORDS = 3

# A card's reference to itself, which XMage writes as the card's name: the text that names an
# ability stops before it, as "{B}: Regenerate this creature." does after "Regenerate".
SELF_REFERENCE: re.Pattern[str] = re.compile(r" this \w+")

# The triggered abilities casting keywords give a permanent, by keyword, as the one thing each
# does: CR 702.74a, evoke: "When this permanent enters, if its evoke cost was paid, its
# controller sacrifices it"; CR 702.175a, offspring: "When this permanent enters, if its
# offspring cost was paid, create a token that's a copy of it, except it's 1/1".
KEYWORD_TRIGGERS: dict[str, str] = {"Evoke": "Sacrifice", "Offspring": "CreateCopies"}

# How a triggered ability's Oracle text starts: its trigger word and the card's reference to
# itself, which XMage's rule text writes as `{this}`.
TRIGGER_START: re.Pattern[str] = re.compile(r"^(When|Whenever|At) (this \w+|.+?) ")


# The bit of a Cast's `costs` for a cast with the escape an effect grants a card in a
# graveyard: Beryl's `Castings::ESCAPE`, the last of the eight.
ESCAPE = 1 << 7


class Cards:
    """The descriptions of the cards one scenario names, looked up by card or face name."""

    def __init__(self, described: Json, oracle: dict[str, str] | None = None) -> None:
        # Each face by its own name, and every face of a card by the card's name.
        self.faces: dict[str, list[Json]] = {}
        # The card each face belongs to, and whether it is a modal card's back face: one
        # that is cast or played as itself, not transformed into.
        self.modal_back: set[str] = set()
        self.grants: list[list[Json]] = []
        for name, card in sorted(described.items()):
            faces = cast(list[Json], cast(Json, card)["faces"])
            self.faces[name] = faces
            for i, face in enumerate(faces):
                face_name = cast(str, face["name"])
                self.faces.setdefault(face_name, [face])
                if i > 0 and not face.get("transformed"):
                    self.modal_back.add(face_name)
            granted = [g for f in faces for g in cast(list[Json], f.get("grants", []))]
            if granted:
                self.grants.append(granted)
        # Each face's Oracle text by face name, from the card store.
        self.oracle: dict[str, str] = oracle or {}

    def abilities(self, name: str) -> list[Json]:
        return [
            a
            for f in self.faces.get(name, [])
            # A face with keywords alone, as Gurmag Angler's delve, lists no abilities.
            for a in cast(list[Json], f.get("abilities", []))
        ]

    def instructions(self, name: str) -> list[Json]:
        return [
            i
            for a in self.abilities(name)
            for i in cast(list[Json], a.get("instructions", []))
        ]

    def instruction(self, name: str, kind: str) -> Json | None:
        """The first instruction of `kind` among what `name`'s abilities do."""
        return next((i for i in self.instructions(name) if i["kind"] == kind), None)

    def ability(self, card: str, how: str, index: int) -> Json | None:
        """The ability a scenario activates as `{how: index}`: its own by index, or the
        `index`th granted. A granted ability is found only when one card in the scenario
        grants abilities: the order of grants from several is a fact of the game."""
        if how == "Own":
            wanted = f"Own {index}"
            return next((a for a in self.abilities(card) if a["index"] == wanted), None)
        if how == "Granted" and len(self.grants) == 1:
            granted = self.grants[0]
            return granted[index] if index < len(granted) else None
        return None

    @staticmethod
    def escape_spell(card: str, costs: int) -> tuple[str, int]:
        """The spell XMage casts for a Cast of `card` paying `costs`, and the options left:
        with the escape an effect grants, its EscapeAbility, "<card> with Escape"."""
        if not costs & ESCAPE:
            return card, costs
        if costs != ESCAPE:
            raise Unsupported(f"escape with another casting option of {card}")
        return f"{card} with Escape", 0

    @staticmethod
    def tap_source(at: str, who: str, card: str, text: str) -> str:
        """The XMage statement that has `who` activate the mana ability of the permanent
        `card` whose text starts with `text` (`BerylScenarioTest.tapSource`)."""
        quoted = [json.dumps(t, ensure_ascii=False) for t in (card, text)]
        return f"tapSource({at}, {who}, {quoted[0]}, {quoted[1]});"

    def instant_mana_text(self, card: str, how: str, index: int) -> str | None:
        """The start of a mana ability's text in XMage where the ability is activated only at
        priority, as Lion's Eye Diamond's "Discard your hand, Sacrifice this artifact: Add
        three mana of any one color. Activate only as an instant." does: its first cost part.
        None for any other ability."""
        ability = self.ability(card, how, index)
        if ability is None or ability["kind"] != "Mana":
            return None
        cost = cast(Json, ability.get("cost", {}))
        parts = cast(list[Json], cost.get("parts", []))
        if not parts or parts[0]["action"] != "DiscardHand":
            return None
        return "Discard your hand"

    def ability_text(self, card: str, how: str, index: int) -> str | None:
        """The start of an activated or loyalty ability's text in XMage, which its
        `activateAbility` matches, or None where the exporter cannot say it."""
        ability = self.ability(card, how, index)
        if ability is None:
            return None
        cost = cast(Json, ability.get("cost", {}))
        parts = cast(list[Json], cost.get("parts", []))
        if ability["kind"] == "Loyalty":
            # XMage writes a loyalty ability's cost as "+N", "0" or "-N" (CR 606.4), where
            # the Oracle text writes a minus sign.
            amount = cast(int, parts[0]["amount"])
            text = f"+{amount}:" if amount > 0 else f"{amount}:"
            oracle = text.replace("-", "\u2212")
        elif ability["kind"] == "Activated":
            cost_written = cost_text(ability, cost, parts)
            if cost_written is None or not cost_written.endswith(":"):
                return cost_written
            text = oracle = cost_written
        else:
            return None
        # A full cost ends at its colon, where another permanent's ability of the same cost
        # would match too, as a land's "{T}: Add" does an ability "{T}: Draw", or Teferi's
        # "+1:" does Jace's. XMage's rule text goes on as the ability's Oracle text does:
        # its first words tell them apart.
        written = [p for p in self.paragraphs(card) if p.startswith(f"{oracle} ")]
        if len(written) != 1:
            return None
        effect = SELF_REFERENCE.split(written[0][len(oracle) :], maxsplit=1)[0]
        return " ".join([text, *effect.split()[:EFFECT_WORDS]])

    def paragraphs(self, card: str) -> list[str]:
        """The paragraphs of the Oracle text of every face of `card`."""
        return [
            p
            for face in self.faces.get(card, [])
            for p in self.oracle.get(cast(str, face["name"]), "").split("\n")
        ]

    def delve_reachable(
        self, setup: Json, seat: object, taps: list[str] | None
    ) -> None:
        """Raises `Unsupported` unless XMage's AI would delve as Beryl did. It pays with mana
        sources first and delves only for what they cannot pay, so a delve is replayable only
        when every untapped mana source of the caster `seat` on the setup's battlefield is
        among the `taps` of the cast."""
        battlefield = cast(
            list[Json],
            cast(list[Json], setup["players"])[cast(int, seat)]["battlefield"],
        )
        sources = [
            b
            for b in battlefield
            if not b["tapped"] and self.mana_text(cast(str, b["card"])) is not None
        ]
        if len(sources) > len(taps or []):
            raise Unsupported(
                "a delve with mana sources left untapped, which XMage's AI taps first"
            )

    def mana_text(self, source: str) -> str | None:
        """The text of a mana source's one mana ability, `{T}: Add` and what it makes, where
        that is all it can tap for. XMage taps its own choice of sources for a payment, so a
        payment tapping only these is replayed."""
        if source in self.modal_back:
            return None
        mana = [a for a in self.abilities(source) if a["kind"] == "Mana"]
        if len(mana) != 1:
            return None
        cost = cast(Json, mana[0].get("cost", {}))
        if cost != {"parts": [{"action": "Tap", "of": "This"}]}:
            return None
        produces = cast(Json, mana[0]["produces"])
        colors = cast(list[str], produces["colors"])
        if len(colors) != 1:
            return None
        return "{T}: Add " + SYMBOLS[colors[0]] * cast(int, produces["amount"])

    def taps_for_one_mana(self, source: str) -> bool:
        """Whether every mana ability of `source` costs only tapping it and makes one mana,
        of a color it may choose: a basic land, Seachrome Coast or Spara's Headquarters, but
        not Mana Crypt, which makes two, or a source that sacrifices itself."""
        if source in self.modal_back:
            return False
        mana = [a for a in self.abilities(source) if a["kind"] == "Mana"]
        return bool(mana) and all(
            a.get("cost") == {"parts": [{"action": "Tap", "of": "This"}]}
            and cast(Json, a["produces"])["amount"] == 1
            for a in mana
        )

    def mana_choice(self, source: str) -> bool:
        """Whether XMage could tap `source` for more than one kind of mana, so that a payment
        with it is left to XMage: several mana abilities, or a modal card's back face, whose
        mana XMage chooses among the card's faces."""
        mana = [a for a in self.abilities(source) if a["kind"] == "Mana"]
        return len(mana) > 1 or (bool(mana) and source in self.modal_back)

    def change_land_types(self) -> bool:
        """Whether a card the scenario names sets or adds basic land types, as Blood Moon or
        Urborg: a land may then tap for mana its own text doesn't write, so no payment's
        mana ability can be named from a land's text."""
        return any(
            a.get("changes_land_types")
            for faces in self.faces.values()
            for f in faces
            for a in cast(list[Json], f.get("abilities", []))
        )

    def sacrificed_for_mana_of_any_color(self, source: str) -> bool:
        """Whether `source` makes mana by sacrificing itself for one of several colors, the
        color of which XMage asks and Beryl's trace does not record."""
        for a in self.abilities(source):
            if a["kind"] != "Mana":
                continue
            parts = cast(list[Json], cast(Json, a.get("cost", {})).get("parts", []))
            colors = cast(list[str], cast(Json, a["produces"])["colors"])
            sacrifice = {"action": "Sacrifice", "of": "This"} in parts
            if sacrifice and len(colors) > 1:
                return True
        return False

    def color_asked(self, source: str) -> bool:
        """Whether tapping `source` for mana makes a choice the oracles ask and Beryl's trace
        does not record: the color of mana it sacrifices itself for, or whether its
        restricted mana pays."""
        return self.sacrificed_for_mana_of_any_color(source) or self.restricted_mana(
            source
        )

    def restricted_mana(self, source: str) -> bool:
        """Whether a mana ability of `source` makes mana that remembers a restriction, "Spend
        this mana only ...", as Cavern of Souls' colored mana: the oracles ask its color and
        whether it pays, which Beryl's trace does not record."""
        return any(
            a["kind"] == "Mana" and a.get("restricted") for a in self.abilities(source)
        )

    def casting(self, card: str) -> list[Json]:
        """The casting options of a card's front face, in the order of Beryl's `costs` bits."""
        faces = self.faces.get(card, [])
        options = cast(list[Json], faces[0].get("casting", [])) if faces else []
        return sorted(options, key=lambda c: cast(int, c["bit"]))

    def overload_bit(self, card: str) -> int | None:
        """The `costs` bit of overload, which XMage casts as a spell of its own, "<name> with
        overload"."""
        for option in self.casting(card):
            if option["keyword"] == "Overload":
                return cast(int, option["bit"])
        return None

    def cast_answers(self, card: str) -> list[tuple[str, str | None]]:
        """For each casting option XMage asks about when the spell is cast, by `costs` bit,
        XMage's answer for paying it and for not paying it. Alternative costs share one menu,
        which XMage may omit if none is available. Additional costs such as kicker and
        offspring are yes or no. Overload has a separate cast command."""
        answers: list[tuple[str, str | None]] = []
        for option in self.casting(card):
            keyword = cast(str, option["keyword"])
            if keyword == "Overload":
                answers.append(("", None))
            elif not option["alternative"]:
                answers.append(("Yes", "No"))
            elif keyword == "Alternative":
                # An alternative cost no keyword names, as Force of Will's: XMage names it by
                # its costs, "Cast with alternative cost: Pay 1 life and ...".
                answers.append(("Cast with alternative cost", "Cast with no"))
            else:
                answers.append((f"Cast with {keyword}", "Cast with no"))
        return answers

    def targets_in_cast(self, card: str) -> bool:
        """Whether a spell's targets go in the cast command rather than the target queue:
        TestPlayer's addTarget cannot choose a permanent for a "spell or permanent" target.
        """
        return any(
            t["kind"] == "SpellOrPermanent"
            for a in self.abilities(card)
            if a["kind"] == "Spell"
            for t in cast(list[Json], a.get("targets", []))
        )

    def targets_abilities(self, card: str) -> bool:
        """Whether a spell targets an activated or triggered ability on the stack, which
        XMage names by its name's prefix, "stack ability (", in the cast command."""
        return any(
            t["kind"] in ("Ability", "ActivatedAbility", "TriggeredAbility")
            for a in self.abilities(card)
            if a["kind"] == "Spell"
            for t in cast(list[Json], a.get("targets", []))
        )

    def unless_pays(self, card: str) -> bool:
        """Whether resolving `card` asks a target's controller to pay a mana cost or have it
        countered. XMage asks even a controller who cannot pay, where Beryl asks nothing
        (CR 118.3): its mana costs can always be paid as far as its question is concerned.
        """
        unless = self.instruction(card, "Unless")
        return unless is not None and "mana" in cast(Json, unless.get("cost", {}))

    def untaps_up_to(self, card: str) -> bool:
        """Whether resolving `card` untaps up to some number of permanents, which XMage first
        offers to do for all its controller's tapped lands ("Include your tapped lands to
        untap?") when there are no more of them than it untaps."""
        return self.instruction(card, "UntapUpTo") is not None

    def may_choose(self, card: str) -> tuple[bool, str] | None:
        """For a resolving ability where Beryl asks whether to choose a card and then which:
        whether XMage asks the yes or no too, and the queue it takes the card from. An
        optional choice among cards XMage asks once, for up to one card; a look at the top of
        a library that may reveal one it asks as a yes or no, then a target."""
        for i in self.instructions(card):
            if i["kind"] == "Choose" and i.get("optional"):
                return (False, "setChoice")
            if i["kind"] == "Dig" and i.get("take") == "MayReveal":
                return (True, "addTarget")
        return None

    def enters_as_copy(self, card: str) -> bool:
        """Whether `card` may enter as a copy of a permanent its controller chooses, which
        Beryl asks as one pick of the permanent or none, and XMage as a yes or no ("use the
        copy effect?") and then the permanent."""
        return any(a.get("enters_as_copy") for a in self.abilities(card))

    def may_search(self, card: str) -> bool:
        """Whether resolving `card` may search a library: a yes or no and then the search,
        which XMage asks as a target from the library even with no card to find, where Beryl,
        with nothing to choose, asks nothing."""
        search = self.instruction(card, "Search")
        return search is not None and bool(search.get("may"))

    def search_by_each_player(self, card: str) -> bool:
        """Whether resolving `card` has each player who lost objects to it search their library
        for that many cards: XMage asks a search of each of them even with nothing to find,
        where Beryl asks nothing."""
        search = self.instruction(card, "Search")
        return (
            search is not None
            and search.get("player") == "EachPlayer"
            and not isinstance(search.get("count"), int)
        )

    def choices_as_targets(self, card: str) -> bool:
        """Whether XMage takes the choices of cards resolving `card` makes from the target
        queue (`CHOICES_AS_TARGETS`)."""
        return any(i["kind"] in CHOICES_AS_TARGETS for i in self.instructions(card))

    def refuse_identical_damage_replacements(self, setup: Json) -> None:
        """Refuses a scenario that starts with two permanents of one card whose replacement
        effect modifies damage, as two Furnaces of Rath: XMage asks the player dealt the damage
        to order the two (`chooseReplacementEffect`), where Beryl applies effects that do the
        same together, asking nothing (CR 616.1e lets the player choose any)."""
        names = [
            cast(str, p["card"])
            for player in cast(list[Json], setup["players"])
            for p in cast(list[Json], player["battlefield"])
        ]
        for name in sorted(set(names)):
            text = self.oracle.get(name, "")
            if (
                names.count(name) > 1
                and "would deal damage" in text
                and "instead" in text
            ):
                raise Unsupported(f"ordering the replacement effects of two {name}")

    def chooses_as_it_enters(self, card: str, *choices: str) -> bool:
        """Whether `card` makes one of the given choices as it enters (CR 614.12a). XMage asks it
        when adding the permanent during setup. Beryl puts it in play with no choice.
        The XMage exporter skips the setup color or creature type prompt, as Painter's
        Servant's or Cavern of Souls', so nothing is recorded."""
        text = self.oracle.get(card, "")
        return any(f"enters, choose a {choice}" in text for choice in choices)

    def scries(self, card: str) -> bool:
        """Whether resolving `card` scries. Beryl chooses the cards for the bottom one at a
        time until done, then the rest from the top down; XMage takes the bottom ones as one
        target list."""
        return self.instruction(card, "Scry") is not None

    def takes(self, card: str) -> int | None:
        """How many cards resolving `card` puts into a hand from a look at the top of a
        library, the rest going to the bottom in any order. Beryl chooses those one at a time,
        then the rest for the bottom one at a time, each under the one before; XMage takes the
        cards for the hand as one target list and the bottom order, in the same sense, from
        the choice queue."""
        dig = self.instruction(card, "Dig")
        take = dig.get("take") if dig is not None else None
        return take if isinstance(take, int) else None

    def flips_a_coin_each_turn(self, card: str) -> bool:
        """Whether `card` has a triggered ability at a step of each turn that flips a coin."""
        return any(
            a["kind"] == "Triggered"
            and a.get("trigger") == "Step"
            and any(
                i["kind"] == "FlipCoin"
                for i in cast(list[Json], a.get("instructions", []))
            )
            for a in self.abilities(card)
        )

    def first_trigger_text(self, card: str) -> str | None:
        """Where `card` has two triggered abilities on one event, which wait together, the
        start of XMage's text of Beryl's first of them by index, which a TriggerOrder answer
        naming the card puts on the stack first: XMage tells the two apart by their text. None
        where the card has no two such triggers, or where Beryl's first is the trigger a
        casting keyword gives (`KEYWORD_TRIGGERS`), which XMage lists first too, so that the
        card's name chooses it.

        Otherwise Beryl's first is the card's first written trigger, as triggers that keywords
        give have no paragraph of their own, and XMage's rule text for it starts as its Oracle
        text does, with the card's reference to itself written `{this}`. The text runs to the
        word after the trigger's comma."""
        triggers = [a for a in self.abilities(card) if a["kind"] == "Triggered"]
        events = [a.get("trigger") for a in triggers]
        repeated = [a for a in triggers if events.count(a.get("trigger")) > 1]
        if not repeated:
            return None
        keywords = {cast(str, c["keyword"]) for c in self.casting(card)}
        made = {KEYWORD_TRIGGERS[k] for k in keywords if k in KEYWORD_TRIGGERS}
        does = [
            i["kind"] for i in cast(list[Json], repeated[0].get("instructions", []))
        ]
        if len(does) == 1 and does[0] in made:
            return None
        paragraphs = self.oracle.get(card, "").split("\n")
        written = next((p for p in paragraphs if TRIGGER_START.match(p)), None)
        if written is None:
            raise Unsupported(f"ordering two triggers of {card} on one event")
        text = TRIGGER_START.sub(r"\1 {this} ", written, count=1)
        trigger, comma, rest = text.partition(", ")
        if not comma:
            raise Unsupported(f"ordering two triggers of {card} on one event")
        return f"{trigger}, {rest.split(' ')[0]}"


def named_cards(entries: Iterable[Json]) -> list[str]:
    """The names of the cards the dump's scenarios describe or start with, each once and
    sorted: with the decks' cards, those the exporters need the Oracle text of, as of a card
    implemented only for scenarios or one complete from the card store, which the dump
    describes only when it has abilities beyond keywords."""
    names: set[str] = set()
    for e in entries:
        names.update(cast(Json, e.get("cards", {})))
        for setup in cast(
            list[Json], cast(Json, e.get("setup", {})).get("players", [])
        ):
            for zone in ("hand", "graveyard", "exile", "library"):
                names.update(cast(list[str], setup.get(zone, [])))
            names.update(
                cast(str, permanent["card"])
                for permanent in cast(list[Json], setup.get("battlefield", []))
            )
    return sorted(names)


# The kinds of answer a player gives while casting a spell or activating an ability, before
# paying for it (601.2b-601.2c): an X, the targets and the cards delve exiles (CR 702.66a, which
# applies once the total cost is known, before the mana is paid).
CAST_CHOICES = ("X", "Target", "TargetCount", "ChooseUpTo")


def payment_taps(
    actions: Sequence[Json], taps: Sequence[Sequence[str]], index: int
) -> list[str] | None:
    """The names of the permanents Beryl tapped to pay for the cast or activation at
    `actions[index]`, from `taps`, the trace's taps by position; None past the trace.

    The payment comes after the choices made while casting (601.2b-601.2f: an X, the
    targets), each a position of its own with nothing tapped yet, so the taps are at the
    first position past those answers. Reading only the next position left a spell with an X
    to the oracle's own payment, which could spend a source a later spell needed."""
    after = index + 1
    while (
        after < len(actions)
        and after + 1 < len(taps)
        and (
            # A scenario's assertion that a choice is open, which has no taps either.
            (
                actions[after]["action"] == "Assert"
                and cast(Json, actions[after]["assertion"])["assert"] == "Decision"
            )
            or (
                actions[after]["action"] == "Answer"
                and actions[after]["kind"] in CAST_CHOICES
            )
        )
        and not taps[after]
    ):
        after += 1
    if after >= len(taps):
        return None
    return list(taps[after])


class Unsupported(Exception):
    """A scenario, or a part of one, outside what the exporter can say in XMage's terms."""


# A turn in which one player controls another (CR 724.1). Both harnesses look a player's
# actions up among that player's own cards and permanents, so neither can yet play the
# controller's actions with the controlled player's resources.
CONTROLLED_TURN: str = (
    "a turn one player controls for another (CR 724), which the harness does not play"
)


def cost_text(ability: Json, cost: Json, parts: list[Json]) -> str | None:
    """The start of an activated ability's text in XMage, from its cost: its keyword where the
    cost is a keyword's (cycling, crew, waterbend), else the mana, `{T}` and the other parts in
    that order, a payment of life as "Pay N life". A full cost ends with its colon; a part whose wording is XMage's own, as what a
    sacrifice names, ends the text before it is written out."""
    keyword = keyword_cost(ability, cost, parts)
    discards_this = [p["action"] for p in parts] == ["Discard"] and parts[0].get(
        "of"
    ) == "This"
    if keyword is not None or discards_this:
        return keyword
    words: list[str] = []
    if "mana" in cost:
        words.append(cast(str, cost["mana"]))
    if {"action": "Tap", "of": "This"} in parts:
        words.append("{T}")
    for part in parts:
        if part["action"] == "Life":
            # CR 119.4: paying life is a cost of its own, written "Pay N life".
            words.append(f"Pay {part['amount']} life")
            continue
        if part.get("of") != "This":
            return None
        action = part["action"]
        if action == "Tap":
            continue
        if action == "Sacrifice":
            return ", ".join([*words, "Sacrifice"])
        if action == "RemoveCounter":
            kind = cast(str, part["counter"])
            counter = COUNTER_WORDS.get(kind, kind.lower())
            return ", ".join([*words, f"Remove a {counter} counter"])
        return None
    return ", ".join(words) + ":" if words else None


def keyword_cost(ability: Json, cost: Json, parts: list[Json]) -> str | None:
    """An activated ability's keyword and its cost where its cost is a keyword's (cycling,
    crew, waterbend) or the ability is a keyword's (equip), as its text starts; None for any
    other."""
    actions = [p["action"] for p in parts]
    instructions = cast(list[Json], ability.get("instructions", []))
    if actions == ["Discard"] and parts[0].get("of") == "This":
        # CR 702.29a: "Cycling [cost]" means "[Cost], Discard this card: Draw a card."
        draws = [i["kind"] for i in instructions] == ["Draw"]
        return "Cycling" if draws and instructions[0].get("count") == 1 else None
    if len(parts) == 1 and "total_power" in parts[0]:
        # CR 702.122a: "Crew N" means "Tap any number of other untapped creatures you control
        # with total power N or greater: This permanent becomes an artifact creature until end
        # of turn."
        return f"Crew {parts[0]['total_power']}"
    kinds = [i["kind"] for i in instructions]
    if not parts and kinds == ["Attach"] and instructions[0].get("objects") == "This":
        # CR 702.6a: "Equip [cost]" means "[Cost]: Attach this permanent to target creature
        # you control. Activate only as a sorcery."
        return f"Equip {cost.get('mana', '')}"
    if any("for_generic" in p for p in parts):
        # Waterbend {N}: each permanent tapped for it pays for {1}.
        return f"waterbend {cost.get('mana', '')}"
    return None
