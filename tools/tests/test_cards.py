import json
import urllib.parse
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
    deck = set(cards.m1_names())
    assert len(deck) == 49
    assert {"Swamp", "Mountain", "Forest"} <= deck
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


def test_a_printing_s_own_name_and_text_replace_the_oracle_s() -> None:
    images: dict[str, str] = {
        v: "https://cards.scryfall.io/x.jpg" for v in cards.IMAGE_VERSIONS
    }

    def face(name: str, printed: str) -> cards.Json:
        return {
            "name": name,
            "printed_name": printed,
            "oracle_text": f"{name} can't be blocked.",
            "printed_text": f"{printed} can't be blocked.",
            "image_uris": images,
        }

    def post(url: str, body: cards.Json) -> cards.Json:
        ids = cast(list[cards.Identifier], body["identifiers"])
        if ids != [{"set": "om1", "collector_number": "30"}]:
            return {"data": []}
        card = {
            "id": "osborn",
            "name": "Norman Osborn // Green Goblin",
            "set": "om1",
            "collector_number": "30",
            "layout": "modal_dfc",
            "card_faces": [
                face("Norman Osborn", "Goben"),
                face("Green Goblin", "Fleem"),
            ],
        }
        return {"data": [card]}

    pinned = {"Goben": {"set": "om1", "collector_number": "30"}}
    (card,) = cast(list[cards.Json], cards.build(["Goben"], pinned, post)["cards"])
    assert card["name"] == "Goben // Fleem"
    faces = cast(list[cards.Json], card["faces"])
    assert [f["name"] for f in faces] == ["Goben", "Fleem"]
    assert faces[0]["oracle_text"] == "Goben can't be blocked."


def test_a_card_scryfall_cannot_find_fails_the_run() -> None:
    def post(url: str, body: cards.Json) -> cards.Json:
        return {"data": [], "not_found": body["identifiers"]}

    with pytest.raises(LookupError, match="Nonexistent Card"):
        cards.build(["Nonexistent Card"], {}, post)


def test_a_card_takes_its_first_real_printing() -> None:
    def printing(set_code: str, **extra: object) -> cards.Json:
        return {"set": set_code, "collector_number": "1", "lang": "en", **extra}

    found: dict[str, list[cards.Json]] = {
        "Damn": [
            printing("pmh2", promo=True, set_type="promo"),
            printing("ovnt", oversized=True),
            printing("amh2", set_type="memorabilia"),
            printing("mh2", set_type="expansion"),
            printing("2x2", set_type="masters"),
        ],
        "Booster Tutor": [
            printing("pal", promo=True),
            printing("unh", set_type="funny"),
        ],
        "Arena Only": [printing("ymid", digital=True, set_type="alchemy")],
    }
    asked: list[str] = []

    def get(url: str) -> cards.Json:
        asked.append(url)
        name = next(n for n in found if urllib.parse.quote_plus(f'!"{n}"') in url)
        return {"data": found[name]}

    first = {n: cards.first_printing(get, n)["set"] for n in found}
    assert first == {"Damn": "mh2", "Booster Tutor": "unh", "Arena Only": "ymid"}
    assert all("order=released" in u and "dir=asc" in u for u in asked)
    # The basics are John Avon's Unstable full arts, with no search.
    assert cards.first_printing(get, "Island") == {
        "set": "ust",
        "collector_number": "213",
    }
    assert len(asked) == 3


def test_pins_keep_what_is_pinned_and_write_back_sorted(tmp_path: Path) -> None:
    pins = {"Goben": {"set": "om1", "collector_number": "30"}}

    def get(url: str) -> cards.Json:
        return {"data": [{"set": "lea", "collector_number": "7", "lang": "en"}]}

    pinned = cards.pin_first_printings(["Goben", "Black Lotus", "Forest"], pins, get)
    assert pinned["Goben"] == pins["Goben"]
    assert pinned["Black Lotus"] == {"set": "lea", "collector_number": "7"}
    path = tmp_path / "printings.csv"
    cards.write_printings(path, pinned)
    assert cards.printings(path) == pinned
    assert path.read_text().splitlines()[1].startswith("Black Lotus,lea,7")
