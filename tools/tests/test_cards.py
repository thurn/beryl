"""`by cards` on tests/fixtures/scryfall-sample.jsonl, a pruned sample of Scryfall's bulk file:
the printings of the decks' cards (`cards.m1_names`), the pinned ones, and every printing of a
few cards the pool rule drops or keeps. After `just cards` changes those cards' records in
data/card-store.tsv, regenerate it with
`just cards --sample tools/tests/fixtures/scryfall-sample.jsonl "Falling Star" Shahrazad
"Fire // Ice" "Kemba's Outfitter" "Goblin Bowling Team"`."""

from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, cards, scryfall
from beryl_tools.cli import main

FIXTURES: Path = Path(__file__).parent / "fixtures"
SAMPLE: Path = FIXTURES / "scryfall-sample.jsonl"


def sample_pool() -> cards.Pool:
    return cards.read_pool(
        scryfall.objects(SAMPLE),
        cards.printings(cards.PRINTINGS),
        cards.exclusions(cards.EXCLUSIONS),
    )


def run(tmp_path: Path) -> Path:
    store = tmp_path / "store.tsv"
    assert main(["cards", "--bulk", str(SAMPLE), "--store", str(store)]) == 0
    return store


def first_face(record: cards.Json) -> cards.Json:
    return cast(list[cards.Json], record["faces"])[0]


def test_the_sample_s_store_holds_the_decks_cards_and_their_tokens(
    tmp_path: Path,
) -> None:
    found, made = cards.listed(run(tmp_path))
    deck = set(cards.m1_names())
    assert len(deck) == 49
    assert {"Swamp", "Mountain", "Forest"} <= deck
    assert {first_face(c)["name"] for c in found} == deck
    # Pinned printings are honored: the basics are John Avon's Unstable full arts.
    island = next(c for c in found if c["name"] == "Island")
    assert (island["set"], island["collector_number"]) == ("ust", "213")
    copy = next(t for t in made if t["name"] == "Copy")
    assert (copy["set"], copy["collector_number"]) == ("t2xm", "31")
    tokens = {
        (t["name"], first_face(t).get("power"), first_face(t).get("toughness"))
        for t in made
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


def test_the_store_answers_every_name_a_card_goes_by(tmp_path: Path) -> None:
    path = run(tmp_path)
    with card_store.Store(path) as store:
        jace = store.card("Jace, Vryn's Prodigy")
        assert jace is not None
        assert jace["name"] == "Jace, Vryn's Prodigy // Jace, Telepath Unbound"
        assert store.card("Jace, Telepath Unbound") == jace
        assert jace["keywords"] == ["Transform", "Mill"]
        front, back = cast(list[card_store.Json], jace["faces"])
        assert (front["side"], back["side"]) == ("front", "back")
        assert back["loyalty"] == "5" and back["color_indicator"] == ["U"]
        (emblem,) = cast(list[card_store.Json], jace["tokens"])
        assert (
            store.tokens(str(emblem["name"]))[0]["scryfall_id"] == emblem["scryfall_id"]
        )

        fire = store.card("Fire")
        assert fire is not None and fire["name"] == "Fire // Ice"
        # A printing with a name of its own is keyed by it, and found by the oracle's.
        ademi = store.card("Ademi of the Silkchutes")
        assert ademi is not None and ademi["set"] == "om1"
        assert store.card("Spectacular Spider-Man") == ademi

        # A card and the token it makes share a name, and stay apart.
        trainer = store.card("Thundertrap Trainer")
        (offspring,) = store.tokens("Thundertrap Trainer")
        assert trainer is not None
        assert "Token" not in trainer["faces"][0]["type_line"]  # type: ignore[index]
        assert offspring["set"] == "tblb"
        # Two printings of a token answer its name.
        treasures = store.tokens("Treasure Token")
        assert {t["set"] for t in treasures} == {"tfra", "tblb"}

        assert store.card("Nonexistent Card") is None
        assert store.tokens("Nonexistent Token") == []
        found = store.cards(["Island", "Fire", "Island"])
        assert [c["name"] for c in found] == ["Island", "Fire // Ice", "Island"]
        with pytest.raises(LookupError, match="Nonexistent Card"):
            store.cards(["Island", "Nonexistent Card"])

        records = list(store)
    kinds = [kind for kind, _ in records]
    assert kinds == sorted(kinds)
    # The tokens are exactly those the cards make.
    made = {
        str(t["scryfall_id"])
        for _, r in records
        for t in cast(list[card_store.Json], r.get("tokens", []))
    }
    assert {str(r["scryfall_id"]) for k, r in records if k == "token"} == made
    ids = [r["scryfall_id"] for _, r in records]
    assert len(ids) == len(set(ids))


def test_the_store_keeps_no_volatile_fields(tmp_path: Path) -> None:
    path = run(tmp_path)
    text = path.read_text()
    for field in ("prices", "legalities", "edhrec_rank", "image_uris", "images"):
        assert f'"{field}"' not in text
    assert "scryfall.io" not in text
    first = path.read_bytes()
    run(tmp_path)
    assert path.read_bytes() == first


def test_the_pool_is_vintage_legality_less_the_exclusions() -> None:
    pool = sample_pool()
    names = {str(c["name"]) for c in pool.shown.values()}
    # Banned in Vintage is still in the pool, but a dexterity card is not.
    assert "Shahrazad" in names
    assert "Falling Star" not in names
    # Neither is an Alchemy card, an un-card, a token or an emblem.
    assert not {"Kemba's Outfitter", "Goblin Bowling Team", "Treasure"} & names
    assert pool.dropped[cards.exclusions(cards.EXCLUSIONS)["Falling Star"]] == 1
    assert pool.dropped[cards.NOT_LEGAL] > 2
    assert pool.not_legal["alchemy"] == 1 and pool.not_legal["funny"] == 1


def printing(set_code: str, number: str, released: str, **extra: object) -> cards.Json:
    return {
        "id": f"{set_code}-{number}",
        "oracle_id": "oracle",
        "name": "Card",
        "set": set_code,
        "collector_number": number,
        "released_at": released,
        "lang": "en",
        "layout": "normal",
        "legalities": {"vintage": "legal"},
        "illustration_id": "art",
        "border_color": "black",
        "frame": "2015",
        "set_type": "expansion",
        "games": ["paper"],
        "image_status": "highres_scan",
        **extra,
    }


def test_printings_go_oldest_first_and_pins_win() -> None:
    newest = printing("fdn", "9", "2024-11-15")
    found = [
        newest,
        printing("m11", "10", "2010-07-16"),
        printing("m11", "9", "2010-07-16"),
    ]
    pool = cards.read_pool(found, {}, {})
    assert pool.shown["oracle"]["id"] == newest["id"]
    pool = cards.read_pool(found, {"Card": ("m11", "10")}, {})
    assert pool.shown["oracle"]["collector_number"] == "10"
    with pytest.raises(LookupError, match="Lost Card"):
        cards.read_pool(found, {"Lost Card": ("lea", "1")}, {})


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
    first = {n: cards.choose(f)["set"] for n, f in found.items()}
    assert first == {"Damn": "mh2", "Booster Tutor": "unh", "Arena Only": "ymid"}


def test_a_card_takes_the_newest_ordinary_printing_of_its_first_art() -> None:
    def printing(set_code: str, art: str = "alpha", **extra: object) -> cards.Json:
        return {
            "set": set_code,
            "collector_number": "1",
            "lang": "en",
            "illustration_id": art,
            "border_color": "black",
            "frame": "1993",
            "set_type": "core",
            "games": ["paper"],
            "image_status": "highres_scan",
            **extra,
        }

    found = [
        printing("lea"),
        printing("leb"),
        printing("2ed", border_color="white"),
        printing("ice", art="ice age"),
        printing("me4", frame="1997", digital=True, games=["mtgo"]),
        printing("dmr", frame="1997", promo=True, promo_types=["release"]),
        # None of these: a promo set's foil and watermark, an all-foil set, The List's
        # stamp, a playtest card, a borderless treatment, the Future Sight frame, Arena
        # renders, a scan waiting for its high-resolution image, and new art.
        printing("g99", frame="1997", promo=True, set_type="promo"),
        printing("v16", frame="2015", set_type="from_the_vault"),
        printing("plst", frame="2015", set_type="masters"),
        printing("sld", frame="2015", promo_types=["playtest"]),
        printing("sld", frame="2015", border_color="borderless"),
        printing("mb2", frame="future"),
        printing("aa1", frame="2015", digital=True, games=["arena"]),
        printing("omb", frame="2015", digital=True, games=["arena", "mtgo"]),
        printing("fdc", frame="2015", image_status="lowres"),
        printing("fdc", art="new", frame="2015"),
    ]
    # Only Magic Online renders a reserved card in a newer frame.
    reserved = found[:5]
    # A modern card never takes a retro frame.
    modern = [
        printing("mh2", frame="2015"),
        printing("h2r", frame="1997", promo_types=["boosterfun"]),
    ]
    chosen = [cards.choose(f)["set"] for f in (found, reserved, modern)]
    assert chosen == ["dmr", "me4", "mh2"]


def test_tokens_take_the_engine_s_names() -> None:
    assert cards.engine_name("Samurai", "Token Creature — Samurai") == "Samurai Token"
    assert cards.engine_name("Map", "Token Artifact — Map") == "Map Token"
    # A named token keeps its name, as does an emblem.
    named = cards.engine_name("Thundertrap Trainer", "Token Creature — Otter Wizard")
    assert named == "Thundertrap Trainer"
    emblem = "Jace, Telepath Unbound Emblem"
    assert cards.engine_name(emblem, "Emblem — Jace") == emblem


def omenpaths(lang: str) -> cards.Json:
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

    return {
        "id": "osborn",
        "oracle_id": "oracle",
        "name": "Norman Osborn // Green Goblin",
        "lang": lang,
        "set": "om1",
        "collector_number": "30",
        "layout": "modal_dfc",
        "card_faces": [face("Norman Osborn", "Goben"), face("Green Goblin", "Fleem")],
    }


def test_a_printing_s_own_name_and_text_replace_the_oracle_s() -> None:
    record = cards.store_record(omenpaths("en"), token=False)
    assert record["name"] == "Goben // Fleem"
    faces = cast(list[cards.Json], record["faces"])
    assert [f["name"] for f in faces] == ["Goben", "Fleem"]
    assert faces[0]["oracle_text"] == "Goben can't be blocked."
    assert cards.aliases(omenpaths("en"), record) == [
        "Goben",
        "Fleem",
        "Norman Osborn // Green Goblin",
        "Norman Osborn",
        "Green Goblin",
    ]
    # A translation's printed name is not the card's.
    translated = cards.store_record(omenpaths("ja"), token=False)
    assert translated["name"] == "Norman Osborn // Green Goblin"


def test_the_committed_store_holds_the_sample_s_records(tmp_path: Path) -> None:
    """data/card-store.tsv is built as the sample's store is: the fixture is not stale."""
    path = run(tmp_path)
    with card_store.Store(path) as sample, card_store.Store() as committed:
        for kind, record in sample:
            name = str(record["name"])
            if kind == "card":
                assert committed.card(name) == record
            else:
                assert record in committed.tokens(name)
