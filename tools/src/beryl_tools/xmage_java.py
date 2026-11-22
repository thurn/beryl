"""XMage's vocabulary for the XMage exporter (`xmage.py`): its steps, counters, mana types and choices,
the faces of cards in the store it needs, the assertions it states Beryl's in, and the Java
test classes the exports are written as."""

from __future__ import annotations

import json
import re
import unicodedata
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

from . import xmage_classes
from .cards import listed
from .card_facts import FACE_DOWN, Cards, Json, Unsupported, setup_face

PACKAGE = "org.mage.test.beryl"
BASE: Path = Path(__file__).parent / "BerylScenarioTest.java"

# The answers XMage takes from the choice queue, by Beryl decision kind: with these, a
# waterbend's permanents, which Beryl asks as a payment whose purpose says so.
CHOICE_KINDS = ("ChooseUpTo", "ChooseObject", "TriggerOrder")
YES_NO_KINDS = ("YesNo", "CastOffer")
# The colors by the number Beryl gives each, in the order of CR 105.1 ("white, blue, black,
# red, and green"), as XMage's color choice names them.
COLORS = ("White", "Blue", "Black", "Red", "Green")
# The name XMage chooses for Beryl's "another card", which stands for every name no card of the
# game has: a real card that no scenario uses.
ANOTHER_CARD = "Black Lotus"


def creature_type(name: object) -> str:
    """The creature type XMage names (`ChoiceCreatureType`) for Beryl's `name` of one: the same,
    but for Beryl's "Another creature type", a type no card of the game has, which is a Kithkin:
    no card a scenario uses is one."""
    text = cast(str, name)
    return "Kithkin" if text == "Another creature type" else text


# Beryl's TurnStep names to XMage's PhaseStep, in turn order.
STEPS: dict[str, str] = {
    "Untap": "UNTAP",
    "Upkeep": "UPKEEP",
    "Draw": "DRAW",
    "PrecombatMain": "PRECOMBAT_MAIN",
    "BeginningOfCombat": "BEGIN_COMBAT",
    "DeclareAttackers": "DECLARE_ATTACKERS",
    "DeclareBlockers": "DECLARE_BLOCKERS",
    "FirstStrikeDamage": "FIRST_COMBAT_DAMAGE",
    "CombatDamage": "COMBAT_DAMAGE",
    "EndOfCombat": "END_COMBAT",
    "PostcombatMain": "POSTCOMBAT_MAIN",
    "End": "END_TURN",
    "Cleanup": "CLEANUP",
}
ORDER: list[str] = list(STEPS)
# The steps every turn has, where a game can stop: combat's middle steps are skipped without
# attackers, and the first-strike step without first strike.
EVERY_TURN = (
    "Untap",
    "Upkeep",
    "Draw",
    "PrecombatMain",
    "BeginningOfCombat",
    "EndOfCombat",
    "PostcombatMain",
    "End",
    "Cleanup",
)

COUNTERS: dict[str, str] = {
    "PlusOne": "P1P1",
    "MinusOne": "M1M1",
    "Loyalty": "LOYALTY",
    "Lore": "LORE",
    "Time": "TIME",
    "Story": "STORY",
    "Ice": "ICE",
    "Chorus": "CHORUS",
}
MANA: dict[str, str] = {
    "White": "WHITE",
    "Blue": "BLUE",
    "Black": "BLACK",
    "Red": "RED",
    "Green": "GREEN",
    "Colorless": "COLORLESS",
}

# Number words as Oracle text writes a count of counters.
NUMBER_WORDS: dict[str, int] = {
    "a": 1,
    "an": 1,
    "one": 1,
    "two": 2,
    "three": 3,
    "four": 4,
    "five": 5,
    "six": 6,
    "seven": 7,
    "eight": 8,
    "nine": 9,
    "ten": 10,
}


def entering_counters(oracle: str, kind: str) -> int:
    """How many counters of Beryl's kind `kind` a permanent with this Oracle text enters with
    ("Dark Depths enters with ten ice counters on it"), which XMage's `addCard` puts on it.
    """
    word = COUNTERS[kind].lower()
    found = re.search(rf"enters with (\w+) {re.escape(word)} counters? on it", oracle)
    return NUMBER_WORDS.get(found.group(1), 0) if found else 0


def xmage_zone(zone: str) -> str:
    """Beryl's zone as the name of XMage's `Zone` constant, which calls exile `EXILED`."""
    return "EXILED" if zone == "Exile" else zone.upper()


# Beryl's zones as the XMage assertion that counts the cards in one.
ZONE_COUNTS: dict[str, str] = {
    "Hand": "assertHandCount",
    "Graveyard": "assertGraveyardCount",
    "Library": "assertLibraryCount",
    "Exile": "assertExileCount",
}


@dataclass(frozen=True)
class Faces:
    """What the exporter needs of the decks' cards in the store: backs, land backs, loyalty,
    toughness, mana cost, Saga faces and each face's Oracle text."""

    back: dict[str, str]
    land_back: dict[str, str]
    loyalty: dict[str, int]
    mana_cost: dict[str, str]
    oracle: dict[str, str]
    toughness: dict[str, int] = field(default_factory=dict)
    sagas: set[str] = field(default_factory=set)

    @staticmethod
    def load(store: Path, also: Iterable[str] = ()) -> Faces:
        back: dict[str, str] = {}
        land_back: dict[str, str] = {}
        loyalty: dict[str, int] = {}
        mana_cost: dict[str, str] = {}
        oracle: dict[str, str] = {}
        toughness: dict[str, int] = {}
        sagas: set[str] = set()
        for card in listed(store, also)[0]:
            faces = cast(list[Json], card["faces"])
            names = [cast(str, f["name"]) for f in faces]
            mana_cost[names[0]] = cast(str, faces[0].get("mana_cost", ""))
            for face, name in zip(faces, names):
                oracle[name] = cast(str, face.get("oracle_text", ""))
                subtypes = cast(str, face["type_line"]).partition(" — ")[2].split()
                if "Saga" in subtypes:
                    sagas.add(name)
                if "loyalty" in face:
                    loyalty[name] = int(cast(str, face["loyalty"]))
                printed = cast(str, face.get("toughness", ""))
                if printed.isdigit():
                    toughness[name] = int(printed)
            if len(faces) > 1:
                back[names[0]] = names[1]
                front_land = "Land" in cast(str, faces[0]["type_line"])
                if "Land" in cast(str, faces[1]["type_line"]) and not front_land:
                    land_back[names[0]] = names[1]
        return Faces(back, land_back, loyalty, mana_cost, oracle, toughness, sagas)


@dataclass
class Export:
    """One scenario as an XMage test method, or why it is unsupported."""

    card: str
    scenario: str
    method: str
    body: list[str] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)
    unsupported: str | None = None


def trample_amount(faces: Faces, attacker: str, blocker: str) -> int | None:
    """The damage XMage's question gives `blocker`, the lone creature blocking `attacker`, if
    `attacker` has trample; None if it has not. XMage asks how a blocked trampler divides its
    damage even with one blocker, as amounts for its blockers with the rest trampling over.
    Beryl asks nothing there: the blocker gets lethal damage, its printed toughness, and the
    rest goes to what the trampler attacks. With two or more blockers Beryl's division is a
    decision of its own, which the exporter does not translate."""
    text = re.sub(r"\([^)]*\)", "", faces.oracle.get(attacker, ""))
    lines = (line.split(",") for line in text.split("\n"))
    if not any("trample" in (k.strip().lower() for k in line) for line in lines):
        return None
    toughness = faces.toughness.get(blocker)
    if toughness is None:
        raise Unsupported(f"a trampler blocked by {blocker}, whose toughness varies")
    return toughness


def xmage_spelling(text: str) -> str:
    """`text` as XMage spells it: accents dropped and Æ written Ae, so `Dandân` is `Dandan`
    (XMage's set files and card classes are ASCII). Other characters stay."""
    folded = unicodedata.normalize("NFKD", text.replace("Æ", "Ae").replace("æ", "ae"))
    return "".join(c for c in folded if not unicodedata.combining(c))


def java_string(text: str) -> str:
    """`text` as a Java string literal, in XMage's spelling: every name the exporter writes
    goes through here, so an accented card is found by XMage."""
    return json.dumps(xmage_spelling(text), ensure_ascii=False)


def class_name(card: str) -> str:
    return xmage_classes.card_class(card.split(" // ")[0]) + "BerylTest"


def method_name(index: int, scenario: str) -> str:
    words = re.sub(r"[^A-Za-z0-9 ]", " ", scenario).split()
    body = "_".join(w.lower() for w in words)[:60].rstrip("_")
    return f"s{index:02d}_{body}"


def outcome_lines(a: Json, player: Callable[[object], str]) -> list[str]:
    """A Beryl outcome assertion as the statements that check it once the game has run. XMage
    ends a loop of mandatory actions as a draw when every player agrees, which
    `BerylScenarioTest` always does (CR 104.4b)."""
    ending = cast(Json, a["outcome"])
    if ending["ending"] == "Win":
        winner = player(ending["player"])
        return [f'org.junit.Assert.assertTrue("{winner} won", {winner}.hasWon());']
    if ending["ending"] != "Draw":
        raise Unsupported(f"a {ending['ending']} outcome assertion")
    return ['org.junit.Assert.assertTrue("a draw", currentGame.isADraw());']


def snapshot_ref(ref: Json, player: Callable[[object], str]) -> str:
    name = ref["name"]
    quoted = "null" if name is None else java_string(cast(str, name))
    zone = xmage_zone(cast(str, ref["zone"]))
    return f"snapshotObject({player(ref['player'])}, Zone.{zone}, {quoted}, {ref['index']})"


def permanent_assertion(a: Json, player: Callable[[object], str]) -> str:
    obj = snapshot_ref(cast(Json, a["object"]), player)
    state = cast(Json, a["state"])
    attachment = state["attached_to"]
    enchanted_player = state.get("enchanted_player")
    host = "null"
    if attachment:
        host = f"{snapshot_ref(cast(Json, attachment), player)}.getId()"
    elif enchanted_player is not None:
        host = f"{player(enchanted_player)}.getId()"
    arrays = [
        java_string("|".join(cast(list[str], state[field])))
        for field in ("colors", "types", "subtypes", "keywords")
    ]
    all_creature_types = str(state["all_creature_types"]).lower()
    pt = state["pt"]
    power_toughness = ", ".join(map(str, cast(list[int], pt))) if pt else "null, null"
    return (
        f"assertPermanentState((mage.game.permanent.Permanent) {obj}, {', '.join(arrays)}, {all_creature_types}, "
        f"{state['damage']}, {host}, {power_toughness})"
    )


def assertion_code(a: Json, player: Callable[[object], str], cards: Cards) -> str:
    """A Beryl assertion as the XMage assertion of the same fact, about the player `who`."""
    what = cast(str, a["assert"])
    who = player(a.get("player"))
    card = cast("str | None", a.get("card"))
    if (
        what == "Count"
        and card is not None
        and a["zone"] not in ("Battlefield", "Stack")
    ):
        card = cards.printed_name(card)
    name = java_string(card) if card is not None else "null"
    count = a.get("count")
    if what == "Life":
        return f"assertLife({who}, {a['life']})"
    if what == "Count":
        zone = cast(str, a["zone"])
        if zone == "Battlefield":
            return f"assertOwnedOnBattlefield({who}, {name}, {count})"
        method = ZONE_COUNTS.get(zone)
        if method is None:
            raise Unsupported(f"counting {zone}")
        if card is None:
            return f"{method}({who}, {count})"
        return f"{method}({who}, {name}, {count})"
    if what == "Tapped":
        return f"assertTappedControlled({who}, {name}, {count})"
    if what == "Controls":
        return f"assertPermanentCount({who}, {name}, {count})"
    if what == "Attacking":
        return f"assertAttackingControlled({who}, {name}, {count})"
    if what == "FaceDown":
        return f"assertFaceDownControlled({who}, {name}, {count})"
    if what == "Pt":
        token = "true" if a["token"] else "false"
        return f"assertPt({who}, {name}, {token}, {a['power']}, {a['toughness']})"
    if what == "Creature":
        creature = "true" if a["creature"] else "false"
        return f"assertCreature({who}, {name}, {creature})"
    if what == "Subtype":
        has = "true" if a["has"] else "false"
        subtype = cast(str, a["subtype"]).upper()
        return f"assertSubtype({who}, {name}, SubType.{subtype}, {has})"
    if what == "Counters":
        kind = COUNTERS[cast(str, a["kind"])]
        return f"assertCountersOn({who}, {name}, CounterType.{kind}, {count})"
    if what == "Colors":
        zone = xmage_zone(cast(str, a["zone"]))
        colors = "".join(f", {java_string(c)}" for c in cast(list[str], a["colors"]))
        return f"assertColorsIn({who}, Zone.{zone}, {name}{colors})"
    if what == "PtIn":
        zone = xmage_zone(cast(str, a["zone"]))
        return f"assertPtIn({who}, Zone.{zone}, {name}, {a['power']}, {a['toughness']})"
    if what == "PermanentState":
        return permanent_assertion(a, player)
    if what == "ManaValue":
        ref = cast(Json, a["object"])
        if ref["zone"] != "Battlefield":
            raise Unsupported("a mana value assertion outside the battlefield")
        return f"assertPermanentManaValue({snapshot_ref(ref, player)}, {a['value']})"
    if what == "StackObject":
        controller = player(a["controller"])
        spell = str(a["spell"]).lower()
        source = java_string(cast(str, a["name"]))
        return f"assertStackObject({a['depth']}, {controller}, {source}, {spell})"
    if what == "Stack":
        return f"assertStackSize({count})"
    if what == "Library":
        top = ", ".join(java_string(n) for n in cast(list[str], a["top"]))
        return f"assertLibraryTop({who}, {top})"
    if what == "ManaPool":
        amounts = ", ".join(map(str, cast(list[int], a["amounts"])))
        return f"assertPool({who}, {amounts})"
    if what == "Mana":
        color = MANA[cast(str, a["color"])]
        return f"assertManaPool({who}, ManaType.{color}, {a['amount']})"
    raise Unsupported(f"a {what} assertion")


IMPORTS = """\
import mage.constants.ManaType;
import mage.constants.PhaseStep;
import mage.constants.SubType;
import mage.constants.Zone;
import mage.counters.CounterType;
import org.junit.Test;
import org.mage.test.player.TestPlayer;
"""


def render_class(card: str, exports: Sequence[Export]) -> str:
    """The test class of one card's supported scenarios."""
    out = [
        f"package {PACKAGE};",
        "",
        IMPORTS,
        f"/** Beryl's scenarios for {card}, generated by `just oracle`. Do not edit. */",
        f"public class {class_name(card)} extends BerylScenarioTest {{",
    ]
    for export in exports:
        if export.unsupported is not None:
            continue
        out += [
            "",
            f"    /** {export.scenario} */",
            "    @Test",
            f"    public void {export.method}() {{",
        ]
        out += [f"        {line}" for line in export.body]
        out.append("    }")
    out.append("}")
    return "\n".join(out) + "\n"


def write(exports: Sequence[Export], directory: Path) -> list[str]:
    """Writes the base class and one test class per card with a supported scenario into
    `directory`, which it empties first. Returns the class names."""
    directory.mkdir(parents=True, exist_ok=True)
    for old in directory.glob("*.java"):
        old.unlink()
    (directory / BASE.name).write_text(BASE.read_text(), newline="\n")
    by_card: dict[str, list[Export]] = {}
    for export in exports:
        by_card.setdefault(export.card, []).append(export)
    classes = []
    for card, group in by_card.items():
        if all(e.unsupported is not None for e in group):
            continue
        name = class_name(card)
        (directory / f"{name}.java").write_text(render_class(card, group), newline="\n")
        classes.append(name)
    return classes


def xmage_cards(checkout: Path) -> set[str]:
    """The card names XMage implements: every set file lists its cards as
    `new SetCardInfo("Name", ...`. A set lists a card XMage has not implemented
    on a commented-out line (`//cards.add(new SetCardInfo("Mind Bend", ...`),
    which does not count."""
    names: set[str] = set()
    pattern = re.compile(r'^\s*cards\.add\(new SetCardInfo\("((?:[^"\\]|\\.)*)"', re.M)
    for path in (checkout / "Mage.Sets" / "src" / "mage" / "sets").glob("*.java"):
        names.update(
            m.group(1) for m in pattern.finditer(path.read_text(errors="replace"))
        )
    return names


def sideboard_setup_cards(who: str, setup: Json, cards: Cards) -> Iterable[str]:
    """Place the scenario's sideboard cards outside the game."""
    for name in cast(list[str], setup.get("sideboard", [])):
        # XMage's test API cannot add to OUTSIDE, so seed its sideboard directly.
        yield "{"
        yield (
            "    mage.cards.Card outsideCard = mage.cards.repository.CardRepository.instance"
            f".findCard({java_string(cards.printed_name(name))}, true).createCard();"
        )
        yield f"    currentGame.loadCards(java.util.Collections.singleton(outsideCard), {who}.getId());"
        yield "    outsideCard.setZone(Zone.OUTSIDE, currentGame);"
        yield f"    {who}.getSideboard().add(outsideCard);"
        yield "}"


def setup_cards(entry: Json) -> Iterable[str]:
    """Every card in a scenario's setup, including the sideboards, by name."""
    for setup in cast(list[Json], cast(Json, entry["setup"])["players"]):
        for zone in ("hand", "graveyard", "exile", "library"):
            yield from cast(list[str], setup[zone])
        yield from cast(list[str], setup.get("sideboard", []))
        for permanent in cast(list[Json], setup["battlefield"]):
            yield cast(str, permanent["card"])


def battlefield_card(
    faces: Faces, cards: Cards, who: str, permanent: Json, start: str, index: int
) -> list[str]:
    """The setup commands for one permanent and the counters it starts with."""
    lines: list[str] = []
    card = cast(str, permanent["card"])
    name = faces.back[card] if setup_face(permanent) else card
    quoted = java_string(name)
    # XMage plays turn 1 from its untap step, which untaps player A's permanents.
    retap = permanent["tapped"] and who == "playerA" and start != "Untap"
    tapped = "true" if permanent["tapped"] and not retap else "false"
    counters = cast("Json | None", permanent["counters"])
    lore = (
        cast(int, counters["count"])
        if counters is not None and counters["kind"] == "Lore"
        else None
    )
    if lore is not None or name in faces.sagas:
        alias = f"beryl_lore_{who}_{index}"
        aliased = java_string(f"{name}@{alias}")
        lines.append(f"addCard(Zone.BATTLEFIELD, {who}, {aliased}, 1, {tapped});")
        lines.append(f"setLoreAtStart({who}, {java_string(alias)}, {lore or 0});")
    else:
        lines.append(f"addCard(Zone.BATTLEFIELD, {who}, {quoted}, 1, {tapped});")
    if retap:
        lines.append(f"tapAtStart({who}, PhaseStep.{STEPS[start]}, {quoted});")
    if cards.chooses_as_it_enters(name, "card name"):
        lines.append(f"setChoice({who}, {java_string(ANOTHER_CARD)});")
    elif cards.chooses_as_it_enters(name, "color", "creature type"):
        lines.append(f'setChoice({who}, "[choice_skip]");')
    elif cards.enters_as_copy(name):
        # The scenario starts with the printed card already on the battlefield.
        lines.append(f"setChoice({who}, false);")
    if lore is not None:
        return lines
    kind, count = counters_to_add(faces, name, counters)
    if count:
        lines.append(
            f"addCounters(1, PhaseStep.{STEPS[start]}, {who}, {quoted}, "
            f"CounterType.{COUNTERS[kind]}, {count});"
        )
    return lines


def counters_to_add(faces: Faces, name: str, counters: Json | None) -> tuple[str, int]:
    """The kind and number of counters to add to the permanent `name` that a scenario starts
    with `counters`, beyond those XMage's `addCard` puts on it: a planeswalker's printed
    loyalty, and the counters a permanent enters with ("enters with ten ice counters").
    """
    kind = cast(str, counters["kind"]) if counters else "Loyalty"
    count = cast(int, counters["count"]) if counters else 0
    oracle = faces.oracle.get(name, "")
    if counters is None:
        entered = max((entering_counters(oracle, k) for k in COUNTERS), default=0)
    elif kind == "Loyalty":
        entered = faces.loyalty.get(name, 0)
    else:
        entered = entering_counters(oracle, kind)
    if count < entered:
        raise Unsupported(f"{name} starts with fewer counters than it enters with")
    return kind, count - entered


# XMage selects these abilities in the cast command instead of asking which
# alternative cost to pay after the command starts.
SPELL_SUFFIX = {
    "Overload": " with overload",
    "Mutate": " using Mutate",
    "Prototype": " using Prototype",
    "Morph": " using Morph",
}


@dataclass(frozen=True)
class SpellCast:
    command: str
    name: str
    # The face whose abilities the spell has. A fused spell has both faces' abilities.
    face: int | None
    costs: int


def spell_cast(cards: Cards, card: str, face: int, costs: int) -> SpellCast:
    """Select the XMage spell ability and leave the remaining costs to its choice queue."""
    name = cards.cast_face(card, face)
    if face == FACE_DOWN:
        morph = cards.casting_bit(card, "Morph", face)
        if costs and (morph is None or costs != 1 << morph):
            raise Unsupported(f"casting option {costs:#b} of face-down {card}")
        return SpellCast(name + SPELL_SUFFIX["Morph"], name, FACE_DOWN, 0)
    command, costs = cards.escape_spell(name, costs)
    for option in cards.casting(card, face):
        keyword = cast(str, option["keyword"])
        mask = 1 << cast(int, option["bit"])
        if not costs & mask or (keyword != "Fuse" and keyword not in SPELL_SUFFIX):
            continue
        if costs != mask:
            raise Unsupported(f"casting option {costs:#b} of {card}")
        if keyword == "Fuse":
            printed = cards.printed_name(card)
            return SpellCast(f"fused {printed}", printed, None, 0)
        selected = FACE_DOWN if keyword == "Morph" else face
        return SpellCast(name + SPELL_SUFFIX[keyword], name, selected, 0)
    return SpellCast(command, name, face, costs)


def activate_on_stack(at: str, who: str, text: str, spell: str | None) -> str:
    """Schedules an activation after the named spell is on the stack, if required."""
    args = f"{at}, {who}, {java_string(text)}"
    if spell is not None:
        args += f", TestPlayer.NO_TARGET, {java_string(spell)}"
    return f"activateAbility({args});"


def turn_face_up_rule(cards: Cards, card: str, printed: str, morphed: bool) -> str:
    """The special action XMage offers to turn a permanent face up."""
    if not morphed:
        return f"{printed}: Turn"
    for option in cards.casting(card):
        if option["keyword"] != "Morph":
            continue
        cost = cast(Json, option.get("turn_up", {}))
        parts = cast(list[Json], cost.get("parts", []))
        if not parts and "mana" in cost:
            return f"{cost['mana']}: Turn"
        if (
            len(parts) == 1
            and "mana" not in cost
            and parts[0]
            == {
                "action": "Discard",
                "of": "Chosen",
                "zone": "Hand",
                "count": 1,
            }
        ):
            return "Discard a card: Turn"
    raise Unsupported(f"paying {card}'s morph cost")


def divided_damage(
    who: str,
    answers: Sequence[Json],
    total: int,
    object_name: Callable[[str], str],
    player: Callable[[object], str],
) -> list[str]:
    """XMage's target commands for damage whose shares are fixed by the target count."""
    names: list[str] = []
    for answer in answers:
        if player(answer["player"]) != who:
            raise Unsupported("another player chooses a divided damage target")
        if answer["kind"] == "TargetCount":
            continue
        pick = cast(Json, answer["pick"])
        if "card" in pick:
            names.append(object_name(cast(str, pick["card"])))
        elif "player" in pick:
            target_player = (
                "PlayerA" if player(pick["player"]) == "playerA" else "PlayerB"
            )
            names.append(f"targetPlayer={target_player}")
        else:
            raise Unsupported("a divided damage target that is not a card or player")
    if not names or total % len(names):
        raise Unsupported("divided damage without a fixed share for each target")
    share = total // len(names)
    return [f"addTargetAmount({who}, {java_string(name)}, {share});" for name in names]


def cast_choices(cards: Cards, who: str, card: str, costs: int, face: int) -> list[str]:
    """The questions XMage asks about casting options during a priority cast."""
    lines: list[str] = []
    if face == FACE_DOWN:
        return []
    questions = cards.cast_answers(card, face)
    if costs >> len(questions):
        raise Unsupported(f"casting option {costs:#b} of {card}")
    alternative = None
    for bit, (paid, unpaid) in enumerate(questions):
        answer = paid if costs >> bit & 1 else unpaid
        if unpaid == "Cast with no":
            if alternative is None or costs >> bit & 1:
                alternative = answer
        elif answer:
            lines.append(f"setChoice({who}, {java_string(answer)});")
    if alternative is not None:
        lines.append(f"setCastCost({who}, {java_string(alternative)});")
    return lines


def offered_cast_choices(cards: Cards, who: str, pick: Json) -> list[str]:
    """Accept a front-face cast and choose its additional costs."""
    card = cast(str, pick["card"])
    if pick["face"] != 0:
        raise Unsupported(f"casting {card}'s back face from an offer")
    costs = cast(int, pick["costs"])
    extra = [
        cast(int, option["bit"])
        for option in cards.casting(card)
        if not option["alternative"]
    ]
    if costs & ~sum(1 << bit for bit in extra):
        raise Unsupported(f"casting option {costs:#b} of {card} from an offer")
    return [f"setChoice({who}, true);"] + [
        f"setChoice({who}, {'true' if costs >> bit & 1 else 'false'});" for bit in extra
    ]
