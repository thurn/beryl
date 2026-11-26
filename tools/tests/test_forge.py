import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, forge, oracle_summary, paths
from beryl_tools.card_facts import Cards, ESCAPE, Unsupported

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
        state["p0battlefield"]
        == "Jace, Vryn's Prodigy|NoETBTrigs|Transformed|Counters:LOYALTY=5"
    )
    assert state["p1battlefield"] == "Jace, Vryn's Prodigy|NoETBTrigs"
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


def test_forge_advances_to_the_snapshot_after_a_silent_turn_change() -> None:
    entry = next(e for e in dumped() if e["card"] == "Thieving Skydiver")
    assertion = {
        "assert": "Count",
        "player": 1,
        "zone": "Hand",
        "card": None,
        "count": 1,
    }
    entry["actions"] = [
        {"action": "Resolve"},
        {"action": "Assert", "assertion": assertion},
    ]
    entry["snapshot"] = 1
    entry["trace"] = [
        {"kind": "Priority", "turn": turn, "step": "PrecombatMain", "flips": []}
        for turn in (1, 2)
    ]
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert skipped == []
    assert steps(scenarios[0]) == [
        {"op": "resolve"},
        {"op": "advance", "turn": 2, "phase": "MAIN1"},
        {"op": "assert", "assertion": assertion},
    ]


@pytest.mark.parametrize("stack", [[], [{"name": "Dark Depths"}]])
def test_first_priority_action_records_whether_setup_stack_should_be_empty(
    stack: list[Json],
) -> None:
    entry = dumped()[0]
    entry["trace"] = [{"kind": "Priority", "stack": stack, "flips": [], "forced": []}]
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert skipped == []
    assert scenarios[0].get("settle_setup_stack", False) == (not stack)


def test_a_decision_answered_for_a_controlled_player_is_scripted_as_theirs() -> None:
    # While player 1 controls player 0 (CR 724.5), the scenario names player 1 as the one who
    # casts, but Forge gives player 0 a controller of its own, which casts from player 0's hand.
    entry = next(e for e in dumped() if e["card"] == "Thieving Skydiver")
    actions = cast(list[Json], entry["actions"])
    actions[0] = {**actions[0], "player": 1}
    entry["trace"] = [
        {"kind": "Priority", "stack": [], "flips": [], "forced": [], "decides_for": 0}
    ]
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert skipped == []
    assert steps(scenarios[0])[0]["player"] == 0


def test_legend_rule_pick_precedes_the_trigger_target() -> None:
    entry = dumped()[0]
    legend = {
        "action": "Answer",
        "kind": "ChooseObject",
        "player": 0,
        "pick": {"pick": "Card", "card": "Jace, Vryn's Prodigy"},
        "purpose": "legend-rule",
    }
    target = {
        "action": "Answer",
        "kind": "Target",
        "player": 0,
        "pick": {"pick": "Player", "player": 1},
    }
    entry["actions"] = [target]
    entry["trace"] = [
        {"kind": "Target", "stack": [{"spell": False}], "flips": [], "forced": [legend]}
    ]
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert skipped == []
    assert scenarios[0]["setup_target"] == 1
    legend_step, target_step = steps(scenarios[0])
    assert legend_step["purpose"] == "legend-rule"
    assert legend_step["pick"] == legend["pick"]
    assert target_step["pick"] == target["pick"]


def test_granted_escape_keeps_delve_and_exile_picks_on_the_cast() -> None:
    entry = dumped()[0]
    spell = {
        "action": "Cast",
        "player": 0,
        "card": "Gurmag Angler",
        "face": 0,
        "costs": ESCAPE,
    }
    delve = {"pick": "Card", "card": "Swamp"}
    exile = {"pick": "Card", "card": "Island"}
    answer = {"action": "Answer", "kind": "ChooseObject", "player": 0}
    entry["actions"] = [spell, {**answer, "pick": exile}, {"action": "Resolve"}]
    entry["trace"] = [
        {"kind": "Priority", "stack": [], "flips": [], "flashback": [spell["card"]]},
        {
            "kind": "ChooseObject",
            "purpose": "exile-cost",
            "flips": [],
            "forced": [
                {**answer, "kind": "ChooseUpTo", "purpose": "delve", "pick": delve}
            ],
        },
        {
            "kind": "Priority",
            "flips": [],
            "forced": [{**answer, "purpose": "exile-cost", "pick": exile}] * 2,
        },
    ]
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert skipped == []
    assert steps(scenarios[0]) == [
        {
            "op": "cast",
            "player": 0,
            "card": "Gurmag Angler",
            "cost": "Escape",
            "delve": [delve],
            "exile-cost": [exile] * 3,
        },
        {"op": "resolve"},
    ]


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
            "reason": "no castable face 1 of Oust",
        }
    ]


def test_preparation_requires_support_for_spell_copies_in_exile() -> None:
    entry = dumped()[0]
    entry["cards"] = {
        "Test Preparer": {"shape": "Prepared", "faces": [{"name": "Test Preparer"}]}
    }
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert scenarios == []
    assert skipped == [
        {
            "card": entry["card"],
            "name": entry["name"],
            "reason": "the Forge exporter does not model prepared spell copies",
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
        key: oracle_summary.Divergence(
            "forge", card, name, "CR 704.5a", "Forge is wrong"
        )
        for key, card, name in (("C / c", "C", "c"), ("D / d", "D", "d"))
    }
    skipped: list[Json] = [{"card": "E", "name": "e", "reason": "casting face 1 of E"}]
    summary = oracle_summary.summarize(results, skipped, allowed)
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
    oracle_summary.divergences(paths.REPO / "data" / "oracle-divergences.json", "forge")


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


def test_an_effect_that_starts_with_its_card_is_picked_by_its_cost() -> None:
    # Forge writes the card's name for "This artifact", and its description starts with the
    # cost, which only this paragraph has.
    oracle = {
        "Made-Up Relic": ORACLE["Made-Up Relic"].replace("Draw a card", "This artifact")
    }
    cards = Cards(SAMPLE, oracle)
    assert forge.ability_selector(cards, "Made-Up Relic", "Own", 1) == "{2}, {T}:"


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
    # A land with no printed mana ability can tap to pay after gaining a basic land type.
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
            "Land Type Grant": {
                "faces": [
                    {
                        "name": "Land Type Grant",
                        "abilities": [{"kind": "Static", "changes_land_types": True}],
                    }
                ]
            },
        }
    )
    activate: Json = {"action": "Activate", "player": 0, "card": "Mine"}
    target: Json = {"action": "Answer", "kind": "Target"}
    actions: list[Json] = [activate, target, {"action": "Resolve"}]
    trace: list[Json] = [
        {"taps": []},
        {"taps": []},
        {"taps": ["Mine", "Swamp", "Coast", "Crypt", "Land Type Grant"]},
    ]
    assert forge.taps(activate, actions, trace, 0, cards, {"Land Type Grant"}) == [
        "Swamp",
        "Coast",
        "Land Type Grant",
    ]


def test_a_source_of_restricted_mana_is_tapped_for_the_mana_beryl_made() -> None:
    # A land like Cavern of Souls makes {C}, or restricted mana of any color: the trace
    # says which it made, so the harness activates the same ability for the same color, and
    # the payment-plan answer that chose it needs no answer of its own.
    tap: Json = {"parts": [{"action": "Tap", "of": "This"}]}
    cavern: Json = {
        "faces": [
            {
                "name": "Cavern",
                "abilities": [
                    {
                        "kind": "Mana",
                        "cost": tap,
                        "produces": {"colors": ["Colorless"], "amount": 1},
                    },
                    {
                        "kind": "Mana",
                        "cost": tap,
                        "restricted": True,
                        "produces": {
                            "colors": ["White", "Blue", "Black", "Red", "Green"],
                            "amount": 1,
                        },
                    },
                ],
            }
        ]
    }
    cards = Cards({"Cavern": cavern})
    cast: Json = {"action": "Cast", "player": 0, "card": "Elk", "costs": 0}
    plan: Json = {"action": "Answer", "kind": "PaymentPlan", "pick": {"pick": "Plan"}}
    actions: list[Json] = [cast, plan, {"action": "Resolve"}]
    made = {"source": "Cavern", "color": "Blue", "restricted": True}
    trace: list[Json] = [
        {"taps": []},
        {"taps": []},
        {"taps": ["Cavern"], "mana": [made]},
    ]
    tapped = forge.taps(cast, actions, trace, 0, cards, set())
    assert tapped == [{"card": "Cavern", "color": "U", "restricted": True}]
    assert forge.replays_plan(plan, paid_as_made=True)
    assert not forge.replays_plan(plan, paid_as_made=False)
    # A trace with no mana of the source can't say which ability it activated.
    trace[2]["mana"] = []
    with pytest.raises(Unsupported):
        forge.taps(cast, actions, trace, 0, cards, set())


def test_a_source_sacrificed_for_two_mana_is_left_to_forge() -> None:
    # A land like Svyelunite Temple has {T}: Add {U} and {T}, Sacrifice: Add {U}{U}.
    # When the trace records amount=1, the harness replays the {T} ability; when amount=2,
    # it leaves the sacrifice ability to Forge's AI instead of tapping for one mana.
    tap: Json = {"parts": [{"action": "Tap", "of": "This"}]}
    sac: Json = {
        "parts": [
            {"action": "Tap", "of": "This"},
            {"action": "Sacrifice", "of": "This"},
        ]
    }
    temple: Json = {
        "faces": [
            {
                "name": "Temple",
                "abilities": [
                    {
                        "kind": "Mana",
                        "cost": tap,
                        "produces": {"colors": ["Blue"], "amount": 1},
                    },
                    {
                        "kind": "Mana",
                        "cost": sac,
                        "produces": {"colors": ["Blue"], "amount": 2},
                    },
                ],
            }
        ]
    }
    cards = Cards({"Temple": temple})
    cast: Json = {"action": "Cast", "player": 0, "card": "Elk", "costs": 0}
    actions: list[Json] = [cast, {"action": "Resolve"}]
    trace_one: list[Json] = [
        {"taps": []},
        {
            "taps": ["Temple"],
            "mana": [
                {"source": "Temple", "color": "Blue", "amount": 1, "restricted": False}
            ],
        },
    ]
    assert forge.taps(cast, actions, trace_one, 0, cards, set()) == [
        {"card": "Temple", "color": "U", "restricted": False}
    ]
    trace_two: list[Json] = [
        {"taps": []},
        {
            "taps": ["Temple"],
            "mana": [
                {"source": "Temple", "color": "Blue", "amount": 2, "restricted": False}
            ],
        },
    ]
    assert forge.taps(cast, actions, trace_two, 0, cards, set()) == []


def test_exact_mana_values_reach_the_harness_with_both_snapshot_references() -> None:
    entry = next(e for e in dumped() if e["card"] == "Jace, Vryn's Prodigy")
    facts = [
        {
            "assert": "ManaValue",
            "object": {
                "zone": "Battlefield",
                "player": seat,
                "name": "Jace, Telepath Unbound",
                "index": 0,
            },
            "value": value,
        }
        for seat, value in [(1, 2), (0, 0)]
    ]
    entry["actions"] = [{"action": "Assert", "assertion": a} for a in facts]
    entry.pop("trace", None)
    entry.pop("snapshot", None)
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert skipped == []
    assert steps(scenarios[0]) == [{"op": "assert", "assertion": a} for a in facts]

    cast(Json, facts[0]["object"])["zone"] = "Graveyard"
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert scenarios == []
    assert skipped[0]["reason"] == "a mana value assertion outside the battlefield"


def test_shared_scenario_is_unsupported() -> None:
    entry: Json = {
        "card": "Island",
        "name": "shared",
        "setup": {"shared": {"library": ["Island"], "graveyard": []}},
    }
    scenarios, skipped = forge.export([entry], card_store.STORE)
    assert scenarios == []
    assert skipped[0]["reason"] == "no oracle plays a shared library"
