import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, forge, paths
from beryl_tools.card_facts import Cards, Unsupported

FIXTURE: Path = Path(__file__).parent / "fixtures" / "scenarios-forge.json"


def dumped() -> list[dict[str, object]]:
    """Three scenarios as `beryl_scenario dump` writes them."""
    return json.loads(FIXTURE.read_text())


Json = dict[str, object]


def steps(scenario: Json) -> list[Json]:
    return cast(list[Json], scenario["steps"])


def exported() -> dict[str, Json]:
    scenarios, skipped = forge.export(dumped(), card_store.STORE)
    assert skipped == []
    return {str(s["card"]): s for s in scenarios}


def lines(state: object) -> dict[str, str]:
    return dict(line.split("=", 1) for line in str(state).splitlines())


def test_setup_is_a_game_state_with_loyalty_spelled_out() -> None:
    state = lines(exported()["Jace, Vryn's Prodigy"]["state"])
    assert state["turn"] == "1"
    assert state["activeplayer"] == "p0"
    assert state["activephase"] == "MAIN1"
    # Transformed, so its loyalty is Jace, Telepath Unbound's printed 5: GameState gives a
    # permanent only the counters it lists.
    assert (
        state["p0battlefield"] == "Jace, Vryn's Prodigy|Transformed|Counters:LOYALTY=5"
    )
    assert state["p1battlefield"] == "Jace, Vryn's Prodigy"
    # Zones are top first in both formats.
    assert state["p0library"] == "Island;Island"
    assert state["p0hand"] == ""


def test_steps_carry_forge_names() -> None:
    skydiver = steps(exported()["Thieving Skydiver"])
    assert skydiver[0] == {
        "op": "cast",
        "player": 0,
        "card": "Thieving Skydiver",
        "cost": "Kicker",
    }
    ops = [s["op"] for s in skydiver]
    assert ops == ["cast", "answer", "resolve", "assert", "assert", "advance", "assert"]
    assert skydiver[5] == {"op": "advance", "turn": 1, "phase": "END_OF_TURN"}
    # The decision assert, about Beryl's own decision surface, is left out and counted.
    assert exported()["Thieving Skydiver"]["unchecked"] == 1


def test_activations_name_forge_abilities() -> None:
    first = steps(exported()["Jace, Vryn's Prodigy"])[0]
    assert first == {
        "op": "activate",
        "player": 0,
        "card": "Jace, Telepath Unbound",
        "ability": "+1:",
    }


def test_coin_flips_come_from_the_trace() -> None:
    crypt = exported()["Mana Crypt"]
    assert crypt["flips"] == [{"player": 0, "won": False}]
    assert steps(crypt)[2] == {
        "op": "assert",
        "assertion": {"assert": "Life", "player": 0, "life": 17},
    }


def test_without_a_trace_flips_are_unknown() -> None:
    entries = [{k: v for k, v in e.items() if k != "trace"} for e in dumped()]
    scenarios, _ = forge.export(entries, card_store.STORE)
    assert all("flips" not in s for s in scenarios)


def test_scenarios_outside_the_vocabulary_are_unsupported() -> None:
    entry = dumped()[0]
    actions = list(cast(list[Json], entry["actions"]))
    actions.insert(
        0, {"action": "Cast", "player": 0, "card": "Oust", "face": 1, "costs": 0}
    )
    scenarios, skipped = forge.export([{**entry, "actions": actions}], card_store.STORE)
    assert scenarios == []
    assert skipped == [
        {
            "card": entry["card"],
            "name": entry["name"],
            "reason": "casting face 1 of Oust",
        }
    ]


def test_summary_applies_the_allowlist() -> None:
    results: list[Json] = [
        {"card": "A", "name": "a", "status": "pass", "failures": []},
        {"card": "B", "name": "b", "status": "fail", "failures": ["life 3"]},
        {"card": "C", "name": "c", "status": "fail", "failures": ["life 4"]},
        {"card": "D", "name": "d", "status": "pass", "failures": []},
        {"card": "F", "name": "f", "status": "unsupported", "reason": "a coin flip"},
    ]
    allowed = {
        key: forge.Divergence("forge", card, name, "CR 704.5a", "Forge is wrong")
        for key, card, name in (("C / c", "C", "c"), ("D / d", "D", "d"))
    }
    skipped: list[Json] = [{"card": "E", "name": "e", "reason": "casting face 1 of E"}]
    summary = forge.summarize(results, skipped, allowed)
    assert (summary["pass"], summary["fail"]) == (2, 1)
    assert (summary["allowlisted"], summary["unsupported"]) == (1, 2)
    # D passes now, so its allowlist entry is stale.
    assert summary["stale_allowlist"] == 1
    scenarios = cast(list[Json], summary["scenarios"])
    statuses = {s["card"]: s["status"] for s in scenarios}
    assert statuses == {
        "A": "pass",
        "B": "fail",
        "C": "allowlisted",
        "D": "pass",
        "E": "unsupported",
        "F": "unsupported",
    }


def test_the_allowlist_parses() -> None:
    forge.divergences(paths.REPO / "data" / "oracle-divergences.json", "forge")


# A made-up card's description, as the dump gives it, with each kind of ability the rules read.
TAP: Json = {"action": "Tap", "of": "This"}
SAMPLE: Json = {
    "Made-Up Relic": {
        "faces": [
            {
                "name": "Made-Up Relic",
                "abilities": [
                    {"index": "Own 0", "kind": "Mana", "cost": {"parts": [TAP]}},
                    {
                        "index": "Own 1",
                        "kind": "Activated",
                        "cost": {"mana": "{2}", "parts": [TAP]},
                    },
                    {
                        "index": "Own 2",
                        "kind": "Activated",
                        "cost": {"parts": [TAP, {"action": "Sacrifice", "of": "This"}]},
                    },
                    {
                        "index": "Own 3",
                        "kind": "Activated",
                        "cost": {
                            "mana": "{1}",
                            "parts": [{"action": "Discard", "of": "This"}],
                        },
                        "instructions": [{"kind": "Draw", "count": 1}],
                    },
                    {
                        "index": "Own 6",
                        "kind": "Activated",
                        "cost": {"parts": [{"action": "Sacrifice", "of": "This"}]},
                    },
                    {
                        "index": "Own 4",
                        "kind": "Loyalty",
                        "cost": {"parts": [{"action": "Loyalty", "amount": -2}]},
                    },
                    {
                        "index": "Own 5",
                        "kind": "Loyalty",
                        "cost": {"parts": [{"action": "Loyalty", "amount": 0}]},
                    },
                ],
                "casting": [
                    {"bit": 1, "keyword": "Kicker", "alternative": False, "cost": {}},
                    {"bit": 0, "keyword": "Evoke", "alternative": True, "cost": {}},
                ],
            }
        ]
    }
}
ORACLE = {
    "Made-Up Relic": "{T}: Add {C}.\n{2}, {T}: Draw a card, then discard a card.\n"
    "{T}, Sacrifice this artifact: Exile target card from a graveyard.\nCycling {1}\n"
    "Sacrifice this artifact: You gain 2 life."
}


def selector(index: int) -> str:
    return forge.ability_selector(Cards(SAMPLE, ORACLE), "Made-Up Relic", "Own", index)


def test_a_loyalty_ability_is_picked_by_its_loyalty_change() -> None:
    assert selector(4) == "-2:"
    assert selector(5) == "0:"


def test_a_keyword_ability_is_picked_by_its_keyword() -> None:
    assert selector(3) == "Cycling"


def test_an_activated_ability_is_picked_by_its_effect_in_oracle_text() -> None:
    # Found by its cost, not the mana ability's "{T}:"; the effect stops at its first clause.
    assert selector(1) == "Draw a card"
    assert selector(2) == "Exile target card"
    # A cost of words alone is no keyword's.
    assert selector(6) == "You gain 2"


def test_an_ability_without_a_paragraph_is_unsupported() -> None:
    with pytest.raises(Unsupported):
        forge.ability_selector(Cards(SAMPLE, {}), "Made-Up Relic", "Own", 1)
    with pytest.raises(Unsupported):
        selector(9)


def test_a_casting_option_is_named_by_its_keyword() -> None:
    cards = Cards(SAMPLE, ORACLE)
    assert forge.casting_option(cards, "Made-Up Relic", 0b01) == "Evoke"
    assert forge.casting_option(cards, "Made-Up Relic", 0b10) == "Kicker"
    with pytest.raises(Unsupported):
        forge.casting_option(cards, "Made-Up Relic", 0b11)


def test_a_payment_names_the_mana_sources_beryl_tapped() -> None:
    # The harness taps these for mana before Forge's AI pays, so it cannot spend another
    # source. A source of several colors is tapped for a color the cost needs, one that
    # makes two mana is left to the AI, and an activated card's own tap is its cost.
    tap: Json = {"parts": [{"action": "Tap", "of": "This"}]}

    def source(*colors: str, amount: int = 1) -> Json:
        abilities = [
            {
                "index": f"Own {i}",
                "kind": "Mana",
                "instructions": [],
                "cost": tap,
                "produces": {"colors": [color], "amount": amount},
            }
            for i, color in enumerate(colors)
        ]
        return {"faces": [{"name": "", "abilities": abilities}]}

    cards = Cards(
        {
            "Swamp": source("Black"),
            "Coast": source("White", "Blue"),
            "Mine": source("Colorless"),
            "Crypt": source("Colorless", amount=2),
        }
    )
    activate: Json = {"action": "Activate", "player": 0, "card": "Mine"}
    target: Json = {"action": "Answer", "kind": "Target"}
    actions: list[Json] = [activate, target, {"action": "Resolve"}]
    trace: list[Json] = [
        {"taps": []},
        {"taps": []},
        {"taps": ["Mine", "Swamp", "Coast", "Crypt"]},
    ]
    assert forge.taps(activate, actions, trace, 0, cards) == ["Swamp", "Coast"]
