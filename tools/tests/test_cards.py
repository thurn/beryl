import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import cards
from beryl_tools.cli import main

FIXTURES: Path = Path(__file__).parent / "fixtures"


def test_the_recording_regenerates_the_data_file(tmp_path: Path) -> None:
    out = tmp_path / "cards.json"
    replay = str(FIXTURES / "scryfall-m1.json")
    argv = ["cards", "--replay", replay, "--output", str(out)]
    assert main(argv) == 0
    assert out.read_bytes() == cards.OUTPUT.read_bytes()

    data = json.loads(out.read_text())
    deck = set(cards.deck_names(cards.DECK))
    assert len(deck) == 29
    assert {c["faces"][0]["name"] for c in data["cards"]} == deck
    tokens = {
        (t["name"], t["faces"][0].get("power"), t["faces"][0].get("toughness"))
        for t in data["tokens"]
    }
    assert {
        ("Map Token", None, None),
        ("Insect Token", "2", "1"),
        ("Human Token", "1", "1"),
        ("Samurai Token", "2", "2"),
        ("Spirit Token", "1", "1"),
        ("Thundertrap Trainer", "1", "1"),
        ("Jace, Telepath Unbound Emblem", None, None),
    } <= tokens


def test_tokens_take_the_engine_s_names() -> None:
    assert cards.engine_name("Samurai", "Token Creature — Samurai") == "Samurai Token"
    assert cards.engine_name("Map", "Token Artifact — Map") == "Map Token"
    # A named token keeps its name, as does an emblem.
    named = cards.engine_name("Thundertrap Trainer", "Token Creature — Otter Wizard")
    assert named == "Thundertrap Trainer"
    emblem = "Jace, Telepath Unbound Emblem"
    assert cards.engine_name(emblem, "Emblem — Jace") == emblem


def test_lookups_go_in_batches_and_honor_pinned_printings() -> None:
    requests: list[list[cards.Identifier]] = []

    def post(url: str, body: cards.Json) -> cards.Json:
        ids = cast(list[cards.Identifier], body["identifiers"])
        requests.append(ids)
        return {"data": [card(i) for i in ids]}

    def card(identifier: cards.Identifier) -> cards.Json:
        name = str(identifier.get("name", identifier.get("set", identifier.get("id"))))
        image = "https://cards.scryfall.io/large/front/0/0/0.jpg"
        return {
            "id": name,
            "name": name,
            "set": "set",
            "collector_number": "1",
            "layout": "normal",
            "image_uris": {v: image for v in cards.IMAGE_VERSIONS},
        }

    names = [f"Card {i}" for i in range(160)] + ["Fire // Ice"]
    pinned = {"Card 3": {"set": "lea", "collector_number": "7"}}
    data = cards.build(names, pinned, post)

    assert [len(r) for r in requests] == [75, 75, 11]
    assert {"set": "lea", "collector_number": "7"} in requests[0]
    assert {"name": "Fire"} in requests[2]
    assert len(cast(list[cards.Json], data["cards"])) == 161


def test_a_card_scryfall_cannot_find_fails_the_run() -> None:
    def post(url: str, body: cards.Json) -> cards.Json:
        return {"data": [], "not_found": body["identifiers"]}

    with pytest.raises(LookupError, match="Nonexistent Card"):
        cards.build(["Nonexistent Card"], {}, post)
