"""XMage's vocabulary for the XMage exporter (`xmage.py`): its steps, counters and mana types,
the faces of cards in the store it needs, the assertions it states Beryl's in, and the Java
test classes the exports are written as."""

from __future__ import annotations

import json
import re
from collections.abc import Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

from . import xmage_classes
from .cards import listed
from .xmage_facts import Json, Unsupported

PACKAGE = "org.mage.test.beryl"
BASE: Path = Path(__file__).parent / "BerylScenarioTest.java"

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
    mana cost and each face's Oracle text."""

    back: dict[str, str]
    land_back: dict[str, str]
    loyalty: dict[str, int]
    mana_cost: dict[str, str]
    oracle: dict[str, str]

    @staticmethod
    def load(store: Path) -> Faces:
        back: dict[str, str] = {}
        land_back: dict[str, str] = {}
        loyalty: dict[str, int] = {}
        mana_cost: dict[str, str] = {}
        oracle: dict[str, str] = {}
        for card in listed(store)[0]:
            faces = cast(list[Json], card["faces"])
            names = [cast(str, f["name"]) for f in faces]
            mana_cost[names[0]] = cast(str, faces[0].get("mana_cost", ""))
            for face, name in zip(faces, names):
                oracle[name] = cast(str, face.get("oracle_text", ""))
                if "loyalty" in face:
                    loyalty[name] = int(cast(str, face["loyalty"]))
            if len(faces) > 1:
                back[names[0]] = names[1]
                front_land = "Land" in cast(str, faces[0]["type_line"])
                if "Land" in cast(str, faces[1]["type_line"]) and not front_land:
                    land_back[names[0]] = names[1]
        return Faces(back, land_back, loyalty, mana_cost, oracle)


@dataclass
class Export:
    """One scenario as an XMage test method, or why it is unsupported."""

    card: str
    scenario: str
    method: str
    body: list[str] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)
    unsupported: str | None = None


def java_string(text: str) -> str:
    return json.dumps(text, ensure_ascii=False)


def class_name(card: str) -> str:
    return xmage_classes.card_class(card.split(" // ")[0]) + "BerylTest"


def method_name(index: int, scenario: str) -> str:
    words = re.sub(r"[^A-Za-z0-9 ]", " ", scenario).split()
    body = "_".join(w.lower() for w in words)[:60].rstrip("_")
    return f"s{index:02d}_{body}"


def assertion_code(a: Json, who: str) -> str:
    """A Beryl assertion as the XMage assertion of the same fact, about the player `who`."""
    what = cast(str, a["assert"])
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
    if what == "Counters":
        kind = COUNTERS[cast(str, a["kind"])]
        return f"assertCountersOn({who}, {name}, CounterType.{kind}, {count})"
    if what == "Stack":
        return f"assertStackSize({count})"
    if what == "Library":
        top = ", ".join(java_string(n) for n in cast(list[str], a["top"]))
        return f"assertLibraryTop({who}, {top})"
    if what == "Mana":
        color = MANA[cast(str, a["color"])]
        return f"assertManaPool({who}, ManaType.{color}, {a['amount']})"
    raise Unsupported(f"a {what} assertion")


IMPORTS = """\
import mage.constants.ManaType;
import mage.constants.PhaseStep;
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
    `new SetCardInfo("Name", ...`."""
    names: set[str] = set()
    pattern = re.compile(r'new SetCardInfo\("((?:[^"\\]|\\.)*)"')
    for path in (checkout / "Mage.Sets" / "src" / "mage" / "sets").glob("*.java"):
        names.update(
            m.group(1) for m in pattern.finditer(path.read_text(errors="replace"))
        )
    return names
