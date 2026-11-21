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
from .card_facts import Cards, Json, Unsupported

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
    toughness, mana cost and each face's Oracle text."""

    back: dict[str, str]
    land_back: dict[str, str]
    loyalty: dict[str, int]
    mana_cost: dict[str, str]
    oracle: dict[str, str]
    toughness: dict[str, int] = field(default_factory=dict)

    @staticmethod
    def load(store: Path, also: Iterable[str] = ()) -> Faces:
        back: dict[str, str] = {}
        land_back: dict[str, str] = {}
        loyalty: dict[str, int] = {}
        mana_cost: dict[str, str] = {}
        oracle: dict[str, str] = {}
        toughness: dict[str, int] = {}
        for card in listed(store, also)[0]:
            faces = cast(list[Json], card["faces"])
            names = [cast(str, f["name"]) for f in faces]
            mana_cost[names[0]] = cast(str, faces[0].get("mana_cost", ""))
            for face, name in zip(faces, names):
                oracle[name] = cast(str, face.get("oracle_text", ""))
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
        return Faces(back, land_back, loyalty, mana_cost, oracle, toughness)


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


def outcome_lines(a: Json) -> list[str]:
    """A Beryl outcome assertion as the statements that check it once the game has run. XMage
    ends a loop of mandatory actions as a draw when every player agrees, which
    `BerylScenarioTest` always does (CR 104.4b). A win is not exported yet."""
    ending = cast(Json, a["outcome"])
    if ending["ending"] != "Draw":
        raise Unsupported(f"a {ending['ending']} outcome assertion")
    return [
        'org.junit.Assert.assertTrue("a draw", playerA.hasDrew() && playerB.hasDrew());'
    ]


def snapshot_ref(ref: Json, player: Callable[[object], str]) -> str:
    name = ref["name"]
    quoted = "null" if name is None else java_string(cast(str, name))
    zone = xmage_zone(cast(str, ref["zone"]))
    return f"snapshotObject({player(ref['player'])}, Zone.{zone}, {quoted}, {ref['index']})"


def permanent_assertion(a: Json, player: Callable[[object], str]) -> str:
    obj = snapshot_ref(cast(Json, a["object"]), player)
    state = cast(Json, a["state"])
    attachment = state["attached_to"]
    host = snapshot_ref(cast(Json, attachment), player) if attachment else "null"
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


def assertion_code(a: Json, player: Callable[[object], str]) -> str:
    """A Beryl assertion as the XMage assertion of the same fact, about the player `who`."""
    what = cast(str, a["assert"])
    who = player(a.get("player"))
    card = cast("str | None", a.get("card"))
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


def setup_cards(entry: Json) -> Iterable[str]:
    """Every card in a scenario's setup, in any zone, by name."""
    for setup in cast(list[Json], cast(Json, entry["setup"])["players"]):
        for zone in ("hand", "graveyard", "exile", "library"):
            yield from cast(list[str], setup[zone])
        for permanent in cast(list[Json], setup["battlefield"]):
            yield cast(str, permanent["card"])


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


def cast_choices(cards: Cards, who: str, card: str, costs: int) -> list[str]:
    """The questions XMage asks about casting options during a priority cast."""
    lines: list[str] = []
    questions = cards.cast_answers(card)
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
