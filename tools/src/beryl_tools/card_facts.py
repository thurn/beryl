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

import re
from collections.abc import Iterable
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
            for a in cast(list[Json], f["abilities"])
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

    def ability_text(self, card: str, how: str, index: int) -> str | None:
        """The start of an activated or loyalty ability's text in XMage, which its
        `activateAbility` matches, or None where the exporter cannot say it."""
        ability = self.ability(card, how, index)
        if ability is None:
            return None
        cost = cast(Json, ability.get("cost", {}))
        parts = cast(list[Json], cost.get("parts", []))
        if ability["kind"] == "Loyalty":
            # XMage writes a loyalty ability's cost as "+N", "0" or "-N" (CR 606.4).
            amount = cast(int, parts[0]["amount"])
            return f"+{amount}:" if amount > 0 else f"{amount}:"
        if ability["kind"] != "Activated":
            return None
        text = cost_text(ability, cost, parts)
        if text is None or not text.endswith(":"):
            return text
        # A full cost ends at its colon, where another permanent's ability of the same cost
        # would match too, as a land's "{T}: Add" does an ability "{T}: Draw". XMage's rule
        # text goes on as the ability's Oracle text does: its first words tell them apart.
        written = [p for p in self.paragraphs(card) if p.startswith(f"{text} ")]
        if len(written) != 1:
            return None
        effect = SELF_REFERENCE.split(written[0][len(text) :], maxsplit=1)[0]
        return " ".join([text, *effect.split()[:EFFECT_WORDS]])

    def paragraphs(self, card: str) -> list[str]:
        """The paragraphs of the Oracle text of every face of `card`."""
        return [
            p
            for face in self.faces.get(card, [])
            for p in self.oracle.get(cast(str, face["name"]), "").split("\n")
        ]

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

    def mana_choice(self, source: str) -> bool:
        """Whether XMage could tap `source` for more than one kind of mana, so that a payment
        with it is left to XMage: several mana abilities, or a modal card's back face, whose
        mana XMage chooses among the card's faces."""
        mana = [a for a in self.abilities(source) if a["kind"] == "Mana"]
        return len(mana) > 1 or (bool(mana) and source in self.modal_back)

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
        XMage's answer for paying it and for not paying it (None where it may not ask). An
        alternative cost is a choice among the ways to cast the spell, offered only when it
        can be paid: always for mana, while some card could pay for another cost. An
        additional cost, as kicker or offspring, is a yes or no."""
        answers: list[tuple[str, str | None]] = []
        for option in self.casting(card):
            keyword = cast(str, option["keyword"])
            if keyword == "Overload":
                answers.append(("", None))
            elif not option["alternative"]:
                answers.append(("Yes", "No"))
            elif cast(Json, option["cost"]).get("parts"):
                answers.append((f"Cast with {keyword}", None))
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
    """The names of the cards the dump's scenarios describe, each once and sorted: with the
    decks' cards, those the exporters need the Oracle text of, as of a card implemented only
    for scenarios."""
    return sorted({n for e in entries for n in cast(Json, e.get("cards", {}))})


class Unsupported(Exception):
    """A scenario, or a part of one, outside what the exporter can say in XMage's terms."""


def cost_text(ability: Json, cost: Json, parts: list[Json]) -> str | None:
    """The start of an activated ability's text in XMage, from its cost: its keyword where the
    cost is a keyword's (cycling, crew, waterbend), else the mana, `{T}` and the other parts in
    that order. A full cost ends with its colon; a part whose wording is XMage's own, as what a
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
