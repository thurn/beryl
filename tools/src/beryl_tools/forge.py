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
import re
import subprocess
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card_store, cards, paths
from .card_facts import (
    as_their_own,
    Cards,
    Unsupported,
    SYMBOLS,
    cost_text,
    keyword_cost,
    made_by,
    named_cards,
    payment_position,
)

HARNESS: Path = paths.TOOLS / "oracles" / "BerylForgeHarness.java"
HARNESS_CLASS = "beryl.BerylForgeHarness"

JsonObject = dict[str, object]


def scripts() -> Path:
    """Forge's card scripts in the cached checkout (read only by the census, census.py)."""
    from . import oracles

    return oracles.checkout(oracles.FORGE) / "forge-gui" / "res" / "cardsfolder"


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
    "Ice": "ICE",
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

# How many words of an ability's effect pick it out of Forge's description, which Forge builds
# from the card's text: enough to tell a card's abilities apart.
EFFECT_WORDS = 3

# Where an effect's words stop: at the end of its first clause, or at the card's reference to
# itself, which Forge writes as the card's name.
EFFECT_END: re.Pattern[str] = re.compile(r"[.,(]| this \w+")


# Asserts about Beryl's decision surface (who decides what among how many candidates) have no
# Forge counterpart: Forge's questions are its own. They are left unchecked and counted.
UNCHECKED_ASSERTS: frozenset[str] = frozenset({"Decision"})


@dataclass(frozen=True)
class Card:
    loyalty: int | None
    oracle: str
    land: bool


def card_faces(store: Path, also: Iterable[str] = ()) -> dict[str, Card]:
    """Every face name of the decks' cards, the cards named in `also` and their tokens in the
    card store with what a GameState line needs, its printed loyalty, which GameState does not
    supply on its own, and its Oracle text, from which Forge builds its description of an
    ability."""
    faces: dict[str, Card] = {}
    listed, tokens = cards.listed(store, also)
    for record in [*listed, *tokens]:
        for face in cast(list[JsonObject], record["faces"]):
            loyalty = face.get("loyalty")
            faces[cast(str, face["name"])] = Card(
                int(cast(str, loyalty)) if loyalty is not None else None,
                cast(str, face.get("oracle_text", "")),
                "Land" in str(face.get("type_line", "")).split("—", 1)[0].split(),
            )
    return faces


def face_name(card: str, face: int, faces: Mapping[str, Card], store: Path) -> str:
    """The name of face `face` of the card named `card`."""
    if face == 0:
        return card
    with card_store.Store(store) as cards_store:
        record = cards_store.card(card)
    if record is not None:
        names = [cast(str, f["name"]) for f in cast(list[JsonObject], record["faces"])]
        if names[0] == card and face < len(names):
            return names[face]
    raise Unsupported(f"no face {face} of {card}")


def state_text(setup: JsonObject, faces: Mapping[str, Card], store: Path) -> str:
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
            permanents.append(permanent_text(permanent, faces, store))
        lines.append(f"p{seat}battlefield={';'.join(permanents)}")
    return "\n".join(lines)


def permanent_text(
    permanent: JsonObject, faces: Mapping[str, Card], store: Path
) -> str:
    card = cast(str, permanent["card"])
    face = cast(int, permanent["face"])
    # These permanents start on the battlefield. Forge's NoETBTrigs setup path
    # places them there without an origin zone, bypassing entry replacements.
    parts = [card, "NoETBTrigs"]
    if permanent["tapped"]:
        parts.append("Tapped")
    if face:
        parts.append("Transformed")
    counters = cast(JsonObject | None, permanent["counters"])
    up = faces.get(face_name(card, face, faces, store))
    if counters is not None:
        parts.append(
            f"Counters:{COUNTERS[cast(str, counters['kind'])]}={counters['count']}"
        )
    elif up is not None and up.loyalty is not None:
        # GameState sets a permanent's counters to the listed ones, none if there are none:
        # a planeswalker needs its printed loyalty spelled out.
        parts.append(f"Counters:LOYALTY={up.loyalty}")
    return "|".join(parts)


def casting_option(cards: Cards, card: str, costs: int) -> str:
    """The casting option a Cast's `costs` pays, as the text that names it in Forge: its
    keyword, which is an alternative cost's name and an optional cost's type there. The harness
    takes one option."""
    options = [o for o in cards.casting(card) if costs == 1 << cast(int, o["bit"])]
    if not options:
        raise Unsupported("paying more than one casting option, or one not described")
    return cast(str, options[0]["keyword"])


def ability_selector(cards: Cards, card: str, how: str, index: int) -> str:
    """Text that picks the ability a scenario activates out of Forge's descriptions of the
    card's abilities, which the harness matches by containment: a loyalty ability's loyalty
    change (CR 606.4), a keyword ability's keyword, and any other activated ability the first
    words of its effect in its paragraph of Oracle text, from which Forge builds its
    description."""
    ability = cards.ability(card, how, index)
    if ability is None:
        raise Unsupported("an ability the scenario's card descriptions do not list")
    cost = cast(JsonObject, ability.get("cost", {}))
    parts = cast(list[JsonObject], cost.get("parts", []))
    if ability["kind"] == "Loyalty":
        amount = cast(int, parts[0]["amount"])
        return f"+{amount}:" if amount > 0 else f"{amount}:"
    if ability["kind"] == "Mana":
        # A mana ability only activated at priority, selected by its first cost part.
        text = cards.instant_mana_text(card, how, index)
        if text is None:
            raise Unsupported("a mana ability that is not activated only at priority")
        return text
    if ability["kind"] != "Activated":
        raise Unsupported("activating an ability that is not an activated ability")
    keyword = keyword_cost(ability, cost, parts)
    if keyword is not None:
        # Forge's description of a keyword ability names the keyword.
        return keyword.split(" ")[0].capitalize()
    text = cost_text(ability, cost, parts)
    if text is None:
        raise Unsupported("an activated ability of a cost the exporter cannot write")
    written = [
        p for p in cards.paragraphs(cards.written_on(card, how)) if p.startswith(text)
    ]
    if len(written) != 1:
        raise Unsupported("an activated ability not found by its cost in Oracle text")
    effect = written[0].partition(": ")[2]
    if effect.startswith("This "):
        # Forge writes the card's name where the effect starts "This land becomes ...", and
        # its description starts with the cost, which only this paragraph has.
        return text
    words = EFFECT_END.split(effect, maxsplit=1)[0].split()[:EFFECT_WORDS]
    if not words:
        raise Unsupported("an activated ability whose effect starts with its card")
    return " ".join(words)


def advance_step(position: JsonObject) -> JsonObject:
    return {
        "op": "advance",
        "turn": position["turn"],
        "phase": PHASES[cast(str, position["step"])],
    }


def step(action: JsonObject, cards: Cards, purpose: object = None) -> JsonObject | None:
    """One Beryl action as a harness step, or None for an assert left unchecked. `cards`
    describes the scenario's cards. `purpose` is what the prompt of the decision an answer
    answers is for, as the trace names it: a kind says only the form of the answer."""
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
            out["cost"] = casting_option(cards, cast(str, action["card"]), costs)
        return out
    if kind == "Activate":
        card = cast(str, action["card"])
        written, source, index = cards.ability_card(card, action["ability"])
        described = cards.ability(written, source, index)
        out = {
            "op": "activate",
            "player": action["player"],
            "card": action["card"],
            "ability": ability_selector(cards, written, source, index),
        }
        if described is not None and described["kind"] == "Mana":
            out["mana"] = True
        return out
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
        return advance_step(action)
    if kind == "Answer":
        pick = cast(JsonObject, action["pick"])
        if pick["pick"] == "Plan":
            raise Unsupported("a chosen payment plan")
        if action["kind"] == "Priority" and pick["pick"] == "Card":
            raise Unsupported("a mana ability activated at priority")
        if purpose == "exile-cost":
            # Forge's AI pays a spell's costs itself (AiCostDecision), asking its controller
            # nothing.
            raise Unsupported("a choice of the cards an exile cost takes")
        return {
            "op": "answer",
            "player": action["player"],
            "kind": action["kind"],
            "purpose": purpose,
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


def taps(
    action: JsonObject,
    actions: Sequence[JsonObject],
    trace: Sequence[JsonObject],
    index: int,
    cards: Cards,
    lands: set[str],
) -> list[str | JsonObject]:
    """The mana sources Beryl tapped to pay for the cast or activation `action`, at
    `actions[index]`, which the harness taps for mana before Forge's AI pays the rest: those
    that only tap for one mana, less the activated card itself. A land with no printed
    mana ability can still tap for one mana when it gains a basic land type.

    A source of restricted mana, as Cavern of Souls, is an object `{"card", "color",
    "restricted"}` with the mana the trace records it made, so that the harness activates
    the same one of its mana abilities for the same color: Forge spends the restricted mana
    only on what its restriction allows, and that spell gets its rider.
    """
    at = payment_position(
        actions, [cast(list[str], t.get("taps", [])) for t in trace], index
    )
    if at is None:
        return []
    found = payment(action, actions, trace, index)
    mana = cast(list[JsonObject], trace[at].get("mana", []))[:]
    land_types_change = cards.change_land_types()
    tapped: list[str | JsonObject] = []
    for name in found:
        if cards.restricted_mana(name):
            made = made_by(name, mana)
            color = SYMBOLS[cast(str, made["color"])].strip("{}")
            tapped.append(
                {"card": name, "color": color, "restricted": made["restricted"]}
            )
        elif cards.taps_for_one_mana(name) or (
            land_types_change
            and name in lands
            and not any(a["kind"] == "Mana" for a in cards.abilities(name))
        ):
            tapped.append(name)
    return tapped


def payment(
    action: JsonObject,
    actions: Sequence[JsonObject],
    trace: Sequence[JsonObject],
    index: int,
) -> list[str]:
    """The names of the permanents Beryl tapped to pay for the cast or activation `action`,
    at `actions[index]`, less the activated card itself."""
    at = payment_position(
        actions, [cast(list[str], t.get("taps", [])) for t in trace], index
    )
    found = [] if at is None else cast(list[str], trace[at].get("taps", []))[:]
    if action["action"] == "Activate" and action["card"] in found:
        found.remove(cast(str, action["card"]))
    return found


def resolved_in_cast(trace: Sequence[JsonObject], index: int) -> int:
    """How many objects resolved while Beryl played the Cast action at `index`: Beryl gives
    no priority to a player who could only pass, so a counterspell cast at a player with no
    answer resolves as it is cast, where the harness waits for a Resolve. Counted where the
    next position is a priority with the stack before the cast, the spell on top, less what
    resolved, and no trigger added."""
    if index + 1 >= len(trace) or trace[index + 1].get("kind") != "Priority":
        return 0
    before, after = (
        [item["name"] for item in cast(list[JsonObject], trace[at].get("stack", []))]
        for at in (index, index + 1)
    )
    cast_onto = len(before) + 1
    if len(after) < cast_onto and before[: len(after)] == after:
        return cast_onto - len(after)
    return 0


def replays_plan(action: JsonObject, paid_as_made: bool) -> bool:
    """Whether `action` answers a choice among the ways to pay ("PaymentPlan") that the
    harness need not answer: the payment before it tapped every one of its sources for the
    mana Beryl made, as for a source of restricted mana, so Forge pays from that mana.
    """
    return (
        paid_as_made
        and action["action"] == "Answer"
        and action["kind"] == "PaymentPlan"
    )


def advance_to_snapshot(
    setup: JsonObject,
    actions: Sequence[JsonObject],
    trace: Sequence[JsonObject],
    snapshot: int | None,
) -> JsonObject | None:
    """The advance Forge needs when Beryl found its next priority in a later phase."""
    if snapshot is None or snapshot >= len(trace):
        return None
    position = trace[snapshot]
    if position["kind"] != "Priority":
        return None
    previous = setup
    for i in reversed(range(snapshot)):
        action = actions[i]
        if action["action"] != "Assert":
            previous = action if action["action"] == "AdvanceTo" else trace[i]
            break
    if (previous["turn"], previous["step"]) == (position["turn"], position["step"]):
        return None
    return advance_step(position)


def export(
    entries: Sequence[JsonObject], store: Path
) -> tuple[list[JsonObject], list[JsonObject]]:
    """The dump's scenarios as harness input, and the unsupported ones with their reasons."""
    faces = card_faces(store, named_cards(entries))
    oracle = {name: face.oracle for name, face in faces.items()}
    lands = {name for name, face in faces.items() if face.land}
    exported: list[JsonObject] = []
    skipped: list[JsonObject] = []
    for entry in entries:
        try:
            if "failure" in entry:
                raise Unsupported(f"Beryl fails it: {entry['failure']}")
            described = Cards(cast(JsonObject, entry.get("cards", {})), oracle)
            described.track_merges(cast(list[JsonObject], entry["actions"]))
            steps: list[JsonObject] = []
            unchecked = 0
            setup_target = None
            snapshot = cast("int | None", entry.get("snapshot"))
            paid_as_made = False
            trace = cast(list[JsonObject], entry.get("trace", []))
            actions = [
                as_their_own(a, trace[i].get("decides_for")) if i < len(trace) else a
                for i, a in enumerate(cast(list[JsonObject], entry["actions"]))
            ]
            setup = cast(JsonObject, entry["setup"])
            snapshot_advance = advance_to_snapshot(setup, actions, trace, snapshot)
            for i, action in enumerate(actions):
                # Include answers Beryl chose silently. Forge can offer more targets or
                # choose a different X, and needs the final Done for each damage division.
                if i < len(trace):
                    steps.extend(
                        cast(
                            JsonObject,
                            step(
                                as_their_own(forced, forced.get("decides_for")),
                                described,
                                forced.get("purpose"),
                            ),
                        )
                        for forced in cast(list[JsonObject], trace[i].get("forced", []))
                        if forced["kind"] in ("Target", "X", "DamageDivision")
                        or forced.get("purpose") == "legend-rule"
                    )
                flashback = (
                    cast(list[str], trace[i].get("flashback", []))
                    if i < len(trace)
                    else []
                )
                if action["action"] == "Cast" and action["card"] in flashback:
                    # The harness finds Forge's flashback spell but fails to play it.
                    raise Unsupported("casting a spell with flashback")
                purpose = trace[i].get("purpose") if i < len(trace) else None
                if replays_plan(action, paid_as_made):
                    # The payment's taps, replayed with the mana each made, chose the plan.
                    unchecked += 1
                    continue
                if (
                    snapshot is not None
                    and i >= snapshot
                    and trace[i].get("kind") != "Priority"
                ):
                    # A final assertion has no next scripted answer to bind it to a
                    # Forge prompt. Checking it at the next priority would see a
                    # different position, usually after the choice had resolved.
                    unchecked += 1
                    continue
                converted = step(action, described, purpose)
                if converted is not None and converted["op"] in ("cast", "activate"):
                    tapped = taps(action, actions, trace, i, described, lands)
                    if tapped:
                        converted["taps"] = tapped
                    paid_as_made = any(isinstance(t, dict) for t in tapped) and len(
                        tapped
                    ) == len(payment(action, actions, trace, i))
                if i == snapshot and snapshot_advance is not None:
                    steps.append(snapshot_advance)
                if converted is None:
                    unchecked += 1
                else:
                    if (
                        converted["op"] == "resolve"
                        and i > 0
                        and actions[i - 1]["action"] == "Cast"
                        and resolved_in_cast(trace, i - 1)
                        and i + 1 < len(trace)
                        and trace[i + 1].get("kind") == "Priority"
                    ):
                        # The spell cast just before resolved as it was cast in Beryl,
                        # which Forge's stack still holds: the harness resolves it first,
                        # down to the stack Beryl had as the Resolve began. Only where the
                        # Resolve ends at a priority: one that stops at a choice while an
                        # ability resolves leaves the rest of the stack to later steps.
                        converted["stack"] = len(
                            cast(list[JsonObject], trace[i].get("stack", []))
                        )
                    if (
                        i == 0
                        and trace
                        and trace[0].get("stack")
                        and converted.get("kind") == "Target"
                    ):
                        setup_target = len(steps)
                    steps.append(converted)
            scenario: JsonObject = {
                "card": entry["card"],
                "name": entry["name"],
                "seed": setup["seed"],
                "state": state_text(setup, faces, store),
                "steps": steps,
                "unchecked": unchecked,
            }
            if setup_target is not None:
                scenario["setup_target"] = setup_target
            if (
                trace
                and trace[0].get("kind") == "Priority"
                and trace[0].get("stack") == []
                and steps
                and steps[0]["op"] in ("cast", "activate", "play_land")
            ):
                scenario["settle_setup_stack"] = True
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
