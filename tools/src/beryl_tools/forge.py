"""The Forge oracle (docs/engine-architecture.md, section 17.2): exports Beryl's scenarios into
Forge's vocabulary and plays them in Forge.

`export` turns each scenario of the `beryl_scenario dump` JSON into a setup in Forge's
`forge.game.GameState` text format and a list of steps for the scripted harness in
tools/oracles/BerylForgeHarness.java, with Beryl's names for steps, zones, counters, optional
costs and abilities mapped to Forge's. A scenario outside that vocabulary is reported as
unsupported with the reason. `run` compiles the harness against the cached Forge build, plays
the exported scenarios and returns one result per scenario.
"""

from __future__ import annotations

import json
import os
import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import paths

HARNESS: Path = paths.TOOLS / "oracles" / "BerylForgeHarness.java"
HARNESS_CLASS = "beryl.BerylForgeHarness"

JsonObject = dict[str, object]

# Beryl's TurnStep variants as Forge's PhaseType names.
PHASES: Mapping[str, str] = {
    "Untap": "UNTAP",
    "Upkeep": "UPKEEP",
    "Draw": "DRAW",
    "PrecombatMain": "MAIN1",
    "BeginningOfCombat": "COMBAT_BEGIN",
    "DeclareAttackers": "COMBAT_DECLARE_ATTACKERS",
    "DeclareBlockers": "COMBAT_DECLARE_BLOCKERS",
    "FirstStrikeDamage": "COMBAT_FIRST_STRIKE_DAMAGE",
    "CombatDamage": "COMBAT_DAMAGE",
    "EndOfCombat": "COMBAT_END",
    "PostcombatMain": "MAIN2",
    "End": "END_OF_TURN",
    "Cleanup": "CLEANUP",
}

# Beryl's CounterKind variants as Forge's CounterEnumType names.
COUNTERS: Mapping[str, str] = {
    "PlusOne": "P1P1",
    "MinusOne": "M1M1",
    "Loyalty": "LOYALTY",
    "Time": "TIME",
    "Story": "STORY",
    "Chorus": "CHORUS",
}

# Beryl's Zone variants as Forge's ZoneType names.
ZONES: Mapping[str, str] = {
    "Library": "Library",
    "Hand": "Hand",
    "Battlefield": "Battlefield",
    "Graveyard": "Graveyard",
    "Exile": "Exile",
    "Stack": "Stack",
}

# Beryl's ManaColor variants as the constants of Forge's forge.card.mana.ManaAtom.
COLORS: Mapping[str, str] = {
    "White": "WHITE",
    "Blue": "BLUE",
    "Black": "BLACK",
    "Red": "RED",
    "Green": "GREEN",
    "Colorless": "COLORLESS",
}

# The casting options a scenario pays (bit 0 of Cast's `costs`, over the face's casting list),
# as the text that names the option in Forge: an alternative cost's spell description, or an
# optional cost's type.
COSTS: Mapping[str, str] = {
    "Overlord of the Mistmoors": "Impending",
    "Damn": "Overload",
    "Winds of Abandon": "Overload",
    "Subtlety": "Evoke",
    "Thieving Skydiver": "Kicker",
    "Thundertrap Trainer": "Offspring",
}

# Beryl's ability indexes (Own over the face's abilities, Granted over those other objects
# grant) as text that picks the ability out of Forge's description of it.
ABILITIES: Mapping[tuple[str, str, int], str] = {
    ("Aang's Iceberg", "Own", 1): "Waterbend",
    ("Currency Converter", "Own", 1): "Draw a card",
    ("Currency Converter", "Own", 2): "Put a card exiled",
    ("Jace, Telepath Unbound", "Own", 0): "+1",
    ("Jace, Telepath Unbound", "Own", 1): "-3",
    ("Jace, Telepath Unbound", "Own", 2): "-9",
    ("Jace, Vryn's Prodigy", "Own", 0): "Draw a card",
    ("Jace, Vryn's Prodigy", "Granted", 0): "Crew",
    ("Jace, Wielder of Mysteries", "Own", 1): "+1",
    ("Jace, Wielder of Mysteries", "Own", 2): "-8",
    ("Jace, the Mind Sculptor", "Own", 0): "+2",
    ("Jace, the Mind Sculptor", "Own", 1): "0:",
    ("Jace, the Mind Sculptor", "Own", 2): "-1",
    ("Jace, the Mind Sculptor", "Own", 3): "-12",
    ("Map Token", "Own", 0): "explore",
    ("Spara's Headquarters", "Own", 4): "Cycling",
    ("Staff of the Storyteller", "Own", 2): "Draw a card",
    ("Strip Mine", "Own", 1): "Destroy target land",
    ("Teferi, Hero of Dominaria", "Own", 0): "+1",
    ("Teferi, Hero of Dominaria", "Own", 1): "-3",
    ("Teferi, Hero of Dominaria", "Own", 2): "-8",
    ("Teferi, Time Raveler", "Own", 1): "+1",
    ("Teferi, Time Raveler", "Own", 2): "-3",
    ("The Wandering Emperor", "Own", 1): "+1",
    ("The Wandering Emperor", "Own", 2): "-1",
    ("The Wandering Emperor", "Own", 3): "-2",
}

# Asserts about Beryl's decision surface (who decides what among how many candidates) have no
# Forge counterpart: Forge's questions are its own. They are left unchecked and counted.
UNCHECKED_ASSERTS: frozenset[str] = frozenset({"Decision"})


class Unsupported(Exception):
    """The scenario is outside what the Forge harness can play."""


@dataclass(frozen=True)
class Card:
    loyalty: int | None


def card_faces(cards_json: Path) -> dict[str, Card]:
    """Every face name in data/cards.json with what a GameState line needs: its printed
    loyalty, which GameState does not supply on its own."""
    data = json.loads(cards_json.read_text())
    faces: dict[str, Card] = {}
    for card in [*data["cards"], *data["tokens"]]:
        for face in card["faces"]:
            loyalty = face.get("loyalty")
            faces[face["name"]] = Card(int(loyalty) if loyalty is not None else None)
    return faces


def face_name(card: str, face: int, faces: Mapping[str, Card], cards_json: Path) -> str:
    """The name of face `face` of the card named `card`."""
    if face == 0:
        return card
    data = json.loads(cards_json.read_text())
    for entry in data["cards"]:
        names = [f["name"] for f in entry["faces"]]
        if names[0] == card and face < len(names):
            return names[face]
    raise Unsupported(f"no face {face} of {card}")


def state_text(setup: JsonObject, faces: Mapping[str, Card], cards_json: Path) -> str:
    """The scenario's setup as a GameState text: one `key=value` line each."""
    players = cast(list[JsonObject], setup["players"])
    lines = [
        f"turn={setup['turn']}",
        f"activeplayer=p{setup['active']}",
        f"activephase={PHASES[cast(str, setup['step'])]}",
        # Every permanent has been under its controller's control since their turn began.
        "removesummoningsickness=true",
    ]
    for seat, player in enumerate(players):
        lines.append(f"p{seat}life={player['life']}")
        for zone in ("hand", "graveyard", "exile", "library"):
            # GameState lists every zone top first, as the dump does.
            names = cast(list[str], player[zone])
            lines.append(f"p{seat}{zone}={';'.join(names)}")
        permanents = []
        for permanent in cast(list[JsonObject], player["battlefield"]):
            permanents.append(permanent_text(permanent, faces, cards_json))
        lines.append(f"p{seat}battlefield={';'.join(permanents)}")
    return "\n".join(lines)


def permanent_text(
    permanent: JsonObject, faces: Mapping[str, Card], cards_json: Path
) -> str:
    card = cast(str, permanent["card"])
    face = cast(int, permanent["face"])
    parts = [card]
    if permanent["tapped"]:
        parts.append("Tapped")
    if face:
        parts.append("Transformed")
    counters = cast(JsonObject | None, permanent["counters"])
    up = faces.get(face_name(card, face, faces, cards_json))
    if counters is not None:
        parts.append(
            f"Counters:{COUNTERS[cast(str, counters['kind'])]}={counters['count']}"
        )
    elif up is not None and up.loyalty is not None:
        # GameState sets a permanent's counters to the listed ones, none if there are none:
        # a planeswalker needs its printed loyalty spelled out.
        parts.append(f"Counters:LOYALTY={up.loyalty}")
    return "|".join(parts)


def step(action: JsonObject) -> JsonObject | None:
    """One Beryl action as a harness step, or None for an assert left unchecked."""
    kind = action["action"]
    if kind == "PlayLand":
        return {"op": "play_land", "player": action["player"], "card": action["card"]}
    if kind == "Cast":
        if action["face"] != 0:
            raise Unsupported(f"casting face {action['face']} of {action['card']}")
        out: JsonObject = {
            "op": "cast",
            "player": action["player"],
            "card": action["card"],
        }
        costs = cast(int, action["costs"])
        if costs:
            label = COSTS.get(cast(str, action["card"]))
            if label is None or costs != 1:
                raise Unsupported(f"casting option {costs} of {action['card']}")
            out["cost"] = label
        return out
    if kind == "Activate":
        ability = cast(dict[str, int], action["ability"])
        [(source, index)] = ability.items()
        selector = ABILITIES.get((cast(str, action["card"]), source, index))
        if selector is None:
            raise Unsupported(f"ability {source} {index} of {action['card']}")
        return {
            "op": "activate",
            "player": action["player"],
            "card": action["card"],
            "ability": selector,
        }
    if kind == "TurnFaceUp":
        # Forge offers turning a manifested permanent face up as its "Unmanifest" ability.
        return {
            "op": "activate",
            "player": action["player"],
            "card": action["card"],
            "ability": "Unmanifest",
        }
    if kind == "Resolve":
        return {"op": "resolve"}
    if kind == "AdvanceTo":
        return {
            "op": "advance",
            "turn": action["turn"],
            "phase": PHASES[cast(str, action["step"])],
        }
    if kind == "Answer":
        pick = cast(JsonObject, action["pick"])
        if pick["pick"] == "Plan":
            raise Unsupported("a chosen payment plan")
        if action["kind"] == "Priority" and pick["pick"] == "Card":
            raise Unsupported("a mana ability activated at priority")
        if action["kind"] == "ExileCost":
            # Forge's AI pays a spell's costs itself (AiCostDecision), asking its controller
            # nothing.
            raise Unsupported("a choice of the cards an exile cost takes")
        return {
            "op": "answer",
            "player": action["player"],
            "kind": action["kind"],
            "pick": pick,
        }
    if kind == "Assert":
        assertion = dict(cast(JsonObject, action["assertion"]))
        if assertion["assert"] in UNCHECKED_ASSERTS:
            return None
        if "zone" in assertion:
            zone = ZONES.get(cast(str, assertion["zone"]))
            if zone is None:
                raise Unsupported(f"an assert about the {assertion['zone']} zone")
            assertion["zone"] = zone
        if assertion["assert"] == "Counters":
            assertion["kind"] = COUNTERS[cast(str, assertion["kind"])]
        if assertion["assert"] == "Mana":
            assertion["color"] = COLORS[cast(str, assertion["color"])]
        return {"op": "assert", "assertion": assertion}
    raise Unsupported(f"action {kind}")


def export(
    entries: Sequence[JsonObject], cards_json: Path
) -> tuple[list[JsonObject], list[JsonObject]]:
    """The dump's scenarios as harness input, and the unsupported ones with their reasons."""
    faces = card_faces(cards_json)
    exported: list[JsonObject] = []
    skipped: list[JsonObject] = []
    for entry in entries:
        try:
            if "failure" in entry:
                raise Unsupported(f"Beryl fails it: {entry['failure']}")
            steps: list[JsonObject] = []
            unchecked = 0
            actions = cast(list[JsonObject], entry["actions"])
            trace = cast(list[JsonObject], entry.get("trace", []))
            for i, action in enumerate(actions):
                # A target or X Beryl chose silently, the only legal one, before this action:
                # the harness takes it where Forge offers more, such as Sink into Stupor's
                # "target spell" offering a triggered ability too, or would take X as 0, as
                # for Thieving Skydiver's kicker {X} with one mana to pay it.
                if i < len(trace):
                    steps.extend(
                        cast(JsonObject, step(forced))
                        for forced in cast(list[JsonObject], trace[i].get("forced", []))
                        if forced["kind"] in ("Target", "X")
                    )
                flashback = (
                    cast(list[str], trace[i].get("flashback", []))
                    if i < len(trace)
                    else []
                )
                if action["action"] == "Cast" and action["card"] in flashback:
                    # The harness finds Forge's flashback spell but fails to play it.
                    raise Unsupported(f"casting {action['card']} with flashback")
                converted = step(action)
                if converted is None:
                    unchecked += 1
                else:
                    steps.append(converted)
            setup = cast(JsonObject, entry["setup"])
            scenario: JsonObject = {
                "card": entry["card"],
                "name": entry["name"],
                "seed": setup["seed"],
                "state": state_text(setup, faces, cards_json),
                "steps": steps,
                "unchecked": unchecked,
            }
            # The dump's trace records Beryl's coin flips; the harness replays them. Without
            # a trace, a scenario that flips a coin is unsupported.
            if "trace" in entry:
                scenario["flips"] = [
                    flip
                    for position in cast(list[JsonObject], entry["trace"])
                    for flip in cast(list[JsonObject], position["flips"])
                ]
            exported.append(scenario)
        except Unsupported as e:
            skipped.append(
                {"card": entry["card"], "name": entry["name"], "reason": str(e)}
            )
    return exported, skipped


@dataclass(frozen=True)
class Divergence:
    """A known disagreement where the oracle is wrong, with the rule that says so."""

    oracle: str
    card: str
    scenario: str
    cr: str
    reason: str


def divergences(path: Path, oracle: str) -> dict[str, Divergence]:
    """The allowlist's entries for `oracle`, keyed by "card / scenario"."""
    entries = cast(list[dict[str, str]], json.loads(path.read_text()))
    found = [Divergence(**e) for e in entries]
    return {f"{d.card} / {d.scenario}": d for d in found if d.oracle == oracle}


def summarize(
    results: Sequence[JsonObject],
    skipped: Sequence[JsonObject],
    allowed: Mapping[str, Divergence],
) -> JsonObject:
    """One status per scenario (pass, fail, allowlisted, unsupported) and their counts. A
    scenario on the allowlist that passes is reported too: its entry is stale."""
    scenarios: list[JsonObject] = []
    for r in results:
        key = f"{r['card']} / {r['name']}"
        entry: JsonObject = {"card": r["card"], "name": r["name"]}
        divergence = allowed.get(key)
        if r["status"] == "unsupported":
            entry["status"] = "unsupported"
            entry["reason"] = r["reason"]
        elif r["status"] == "pass":
            entry["status"] = "pass"
            if divergence is not None:
                entry["stale_allowlist"] = True
        elif divergence is not None:
            entry["status"] = "allowlisted"
            entry["cr"] = divergence.cr
            entry["reason"] = divergence.reason
        else:
            entry["status"] = "fail"
            entry["failures"] = r["failures"]
        scenarios.append(entry)
    for u in skipped:
        scenarios.append(
            {
                "card": u["card"],
                "name": u["name"],
                "status": "unsupported",
                "reason": u["reason"],
            }
        )
    counts = {
        status: sum(1 for e in scenarios if e["status"] == status)
        for status in ("pass", "fail", "allowlisted", "unsupported")
    }
    stale = sum(1 for e in scenarios if e.get("stale_allowlist"))
    return {**counts, "stale_allowlist": stale, "scenarios": scenarios}


def attention(summary: JsonObject) -> list[JsonObject]:
    """The scenarios of a summary that need a look: all but the clean passes."""
    scenarios = cast(list[JsonObject], summary["scenarios"])
    return [s for s in scenarios if s["status"] != "pass" or s.get("stale_allowlist")]


def compile_harness(java_home: Path, jar: Path, out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            str(java_home / "bin" / "javac"),
            "-nowarn",
            "-encoding",
            "UTF-8",
            "-d",
            str(out),
            "-cp",
            str(jar),
            str(HARNESS),
        ],
        check=True,
        capture_output=True,
        text=True,
    )


def play(
    java_home: Path,
    jar: Path,
    classes: Path,
    cwd: Path,
    scenarios: Path,
    results: Path,
    log: Path,
) -> list[JsonObject]:
    """Plays the exported scenarios in Forge; returns the harness's result per scenario."""
    with log.open("w") as f:
        code = subprocess.run(
            [
                str(java_home / "bin" / "java"),
                "-Djava.awt.headless=true",
                "-cp",
                os.pathsep.join([str(jar), str(classes)]),
                HARNESS_CLASS,
                str(scenarios),
                str(results),
            ],
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            stdout=f,
            stderr=subprocess.STDOUT,
            check=False,
        ).returncode
    if code != 0:
        tail = "\n".join(log.read_text().splitlines()[-30:])
        raise RuntimeError(f"the Forge harness exited {code}; see {log}\n{tail}")
    return cast(list[JsonObject], json.loads(results.read_text()))
