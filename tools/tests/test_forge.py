import json
from pathlib import Path
from typing import cast

from beryl_tools import forge, paths

FIXTURE: Path = Path(__file__).parent / "fixtures" / "scenarios-forge.json"
CARDS: Path = paths.REPO / "data" / "cards.json"


def dumped() -> list[dict[str, object]]:
    """Three scenarios as `beryl_scenario dump` writes them."""
    return json.loads(FIXTURE.read_text())


Json = dict[str, object]


def steps(scenario: Json) -> list[Json]:
    return cast(list[Json], scenario["steps"])


def exported() -> dict[str, Json]:
    scenarios, skipped = forge.export(dumped(), CARDS)
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
        "ability": "+1",
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
    scenarios, _ = forge.export(entries, CARDS)
    assert all("flips" not in s for s in scenarios)


def test_scenarios_outside_the_vocabulary_are_unsupported() -> None:
    entry = dumped()[0]
    actions = list(cast(list[Json], entry["actions"]))
    actions.insert(
        0, {"action": "Cast", "player": 0, "card": "Oust", "face": 1, "costs": 0}
    )
    scenarios, skipped = forge.export([{**entry, "actions": actions}], CARDS)
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
