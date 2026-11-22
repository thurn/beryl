"""Both oracle exporters select the spell Beryl casts from a scenario dump."""

from __future__ import annotations

from dataclasses import dataclass
from typing import cast

import pytest

from beryl_tools import card_store, forge, xmage_export, xmage_java
from beryl_tools.card_facts import FACE_DOWN, Json


@dataclass(frozen=True)
class CastingCase:
    card: str
    printed: str
    faces: tuple[str, ...]
    shape: str
    face: int
    lands: tuple[str, ...]
    command: str
    spell: str | None = None
    cost: str | None = None
    divided: int | None = None
    targets: tuple[str, ...] = ()
    assertions: tuple[tuple[str, object], ...] = ()


CASES: list[CastingCase] = [
    CastingCase(
        "Fire // Ice",
        "Fire // Ice",
        ("Fire", "Ice"),
        "Split",
        0,
        ("Mountain", "Mountain"),
        "Fire",
        spell="Fire",
        divided=2,
        targets=("opponent",),
        assertions=(("life", 18),),
    ),
    CastingCase(
        "Wear",
        "Wear // Tear",
        ("Wear", "Tear"),
        "Split",
        0,
        ("Mountain", "Mountain"),
        "Wear",
        spell="Wear",
        targets=("Ornithopter",),
        assertions=(("graveyard", "Ornithopter"),),
    ),
    CastingCase(
        "Wear",
        "Wear // Tear",
        ("Wear", "Tear"),
        "Split",
        1,
        ("Plains",),
        "Tear",
        spell="Tear",
        targets=("Glorious Anthem",),
        assertions=(("graveyard", "Glorious Anthem"),),
    ),
    CastingCase(
        "Wear",
        "Wear // Tear",
        ("Wear", "Tear", "Wear // Tear"),
        "Split",
        2,
        ("Mountain", "Mountain", "Plains"),
        "fused Wear // Tear",
        cost="Fuse",
        targets=("Ornithopter", "Glorious Anthem"),
        assertions=(("graveyard", "Ornithopter"), ("graveyard", "Glorious Anthem")),
    ),
    CastingCase(
        "Bonecrusher Giant",
        "Bonecrusher Giant",
        ("Bonecrusher Giant", "Stomp"),
        "Adventure",
        1,
        ("Mountain", "Mountain"),
        "Stomp",
        spell="Stomp",
        targets=("opponent",),
        assertions=(("life", 18),),
    ),
    CastingCase(
        "Alrund, God of the Cosmos",
        "Alrund, God of the Cosmos",
        ("Alrund, God of the Cosmos", "Hakka, Whispering Raven"),
        "Modal",
        1,
        ("Island", "Island"),
        "Hakka, Whispering Raven",
        spell="Hakka, Whispering Raven",
        assertions=(("permanent", "Hakka, Whispering Raven"),),
    ),
    CastingCase(
        "Blitz Automaton",
        "Blitz Automaton",
        ("Blitz Automaton",),
        "Single",
        0,
        ("Mountain",) * 3,
        "Blitz Automaton using Prototype",
        cost="Prototype",
        assertions=(("pt", (3, 2)),),
    ),
    CastingCase(
        "Pine Walker",
        "Pine Walker",
        ("Pine Walker",),
        "Single",
        FACE_DOWN,
        ("Forest",) * 3,
        "Pine Walker using Morph",
        cost="Morph",
        assertions=(("face_down", "Pine Walker"),),
    ),
]


def permanent(card: str) -> Json:
    return {"card": card, "tapped": False, "face": 0, "counters": None}


def assertion(case: CastingCase, kind: str, value: object) -> Json:
    match kind:
        case "life":
            return {"assert": "Life", "player": 1, "life": value}
        case "permanent":
            return {"assert": "Controls", "player": 0, "card": value, "count": 1}
        case "pt":
            power, toughness = cast(tuple[int, int], value)
            return {
                "assert": "Pt",
                "player": 0,
                "card": case.card,
                "token": False,
                "power": power,
                "toughness": toughness,
            }
        case "face_down":
            return {"assert": "FaceDown", "player": 0, "card": value, "count": 1}
        case "graveyard":
            return {
                "assert": "Count",
                "player": 1,
                "zone": "Graveyard",
                "card": value,
                "count": 1,
            }
        case _:
            raise AssertionError(kind)


def scenario(case: CastingCase) -> Json:
    faces = [dict[str, object](name=name) for name in case.faces]
    if case.divided is not None:
        faces[case.face]["abilities"] = [
            {
                "kind": "Spell",
                "instructions": [{"kind": "DealDamage", "divided": case.divided}],
            }
        ]
    if case.cost is not None:
        index = 0 if case.face == FACE_DOWN else case.face
        faces[index]["casting"] = [
            {"bit": 0, "keyword": case.cost, "alternative": True}
        ]
    actions: list[Json] = [
        {
            "action": "Cast",
            "player": 0,
            "card": case.card,
            "face": case.face,
            "costs": int(case.cost is not None),
        }
    ]
    base: Json = {
        "turn": 1,
        "step": "PrecombatMain",
        "decider": 0,
        "kind": "Priority",
        "stack": [],
        "flips": [],
        "taps": [],
        "forced": [],
    }
    trace = [base]
    for target in case.targets:
        pick = (
            dict[str, object](pick="Player", player=1)
            if target == "opponent"
            else dict[str, object](pick="Card", card=target)
        )
        actions.append(
            {"action": "Answer", "kind": "Target", "player": 0, "pick": pick}
        )
        trace.append({**base, "kind": "Target"})
    actions.append({"action": "Resolve"})
    name = (
        ""
        if case.face == FACE_DOWN
        else case.printed if case.cost == "Fuse" else case.faces[case.face]
    )
    trace.append(
        {
            **base,
            "decider": 1,
            "stack": [{"name": name, "spell": True, "controller": 0}],
        }
    )
    for kind, value in case.assertions:
        actions.append({"action": "Assert", "assertion": assertion(case, kind, value)})
        trace.append(base)
    if case.shape == "Split":
        actions.append(
            {
                "action": "Assert",
                "assertion": {
                    "assert": "Count",
                    "player": 0,
                    "zone": "Graveyard",
                    "card": case.card,
                    "count": 1,
                },
            }
        )
        trace.append(base)
    empty: Json = {"life": 20, "hand": [], "graveyard": [], "exile": [], "library": []}
    return {
        "card": case.card,
        "name": case.command,
        "cards": {case.card: {"shape": case.shape, "faces": faces}},
        "setup": {
            "turn": 1,
            "step": "PrecombatMain",
            "active": 0,
            "seed": 0,
            "players": [
                {
                    **empty,
                    "hand": [case.card],
                    "battlefield": [permanent(land) for land in case.lands],
                },
                {
                    **empty,
                    "battlefield": [
                        permanent(t) for t in case.targets if t != "opponent"
                    ],
                },
            ],
        },
        "actions": actions,
        "trace": trace,
    }


@pytest.mark.parametrize("case", CASES)
def test_export_casting_modes(case: CastingCase) -> None:
    entry = scenario(case)
    faces = xmage_java.Faces.load(card_store.STORE, [case.card])
    known = {case.printed, *case.lands, *(set(case.targets) - {"opponent"})}
    java = xmage_export.export_one(entry, 1, faces, known)
    assert java.unsupported is None
    assert f'addCard(Zone.HAND, playerA, "{case.printed}");' in java.body
    assert (
        f'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerA, "{case.command}");'
        in java.body
    )

    if case.shape == "Split":
        assert any(
            f'assertGraveyardCount(playerA, "{case.printed}", 1)' in line
            for line in java.body
        )

    exported, skipped = forge.export([entry], card_store.STORE)
    assert not skipped
    assert f"p0hand={case.printed}" in cast(str, exported[0]["state"]).splitlines()
    step = cast(list[Json], exported[0]["steps"])[0]
    expected: Json = {"op": "cast", "player": 0, "card": case.card}
    if case.spell is not None:
        expected["spell"] = case.spell
    if case.cost is not None:
        expected["cost"] = case.cost
    if case.divided is not None:
        expected["divided"] = case.divided
        assert (
            f'addTargetAmount(playerA, "targetPlayer=PlayerB", {case.divided});'
            in java.body
        )
    assert step == expected


def test_an_alternate_face_uses_its_own_casting_cost_bits() -> None:
    case = next(case for case in CASES if case.shape == "Modal")
    entry = scenario(case)
    cards = cast(dict[str, Json], entry["cards"])
    front, back = cast(list[Json], cards[case.card]["faces"])
    front["casting"] = [{"bit": 0, "keyword": "Evoke", "alternative": True}]
    back["casting"] = [{"bit": 0, "keyword": "Kicker", "alternative": False}]
    cast(list[Json], entry["actions"])[0]["costs"] = 1

    faces = xmage_java.Faces.load(card_store.STORE, [case.card])
    java = xmage_export.export_one(entry, 1, faces, {case.card, *case.lands})
    assert java.unsupported is None
    assert 'setChoice(playerA, "Yes");' in java.body
    assert not any("setCastCost(" in line for line in java.body)

    exported, skipped = forge.export([entry], card_store.STORE)
    assert not skipped
    assert cast(list[Json], exported[0]["steps"])[0]["cost"] == "Kicker"
