"""The cards complete from the card store (store_cards.py), `card path` for one, and the set
crates written from data/set-groups.json (card_crates.py)."""

import argparse
from pathlib import Path

import pytest

from beryl_tools import card, card_crates, card_store, set_groups, store_cards

KEYWORDS = """bitset! {
    /// Keyword abilities.
    Keywords(u64) {
        /// First strike.
        FIRST_STRIKE = 0,
        FLYING = 8,
        VIGILANCE = 10,
    }
}

bitset! {
    Other(u8) {
        TRAMPLE = 0,
    }
}
"""


@pytest.fixture
def known(tmp_path: Path) -> dict[str, str]:
    source = tmp_path / "characteristics.rs"
    source.write_text(KEYWORDS)
    return store_cards.keywords(source)


def creature(
    name: str, text: str = "", cost: str = "{1}{G}", type_line: str = "Creature — Bear"
) -> card_store.Json:
    return {
        "name": name,
        "first_set": "lea",
        "layout": "normal",
        "faces": [
            {
                "name": name,
                "mana_cost": cost,
                "type_line": type_line,
                "oracle_text": text,
                "power": "2",
                "toughness": "2",
            }
        ],
    }


def test_the_keywords_are_the_engines_bitset_constants(known: dict[str, str]) -> None:
    assert known == {
        "first strike": "FIRST_STRIKE",
        "flying": "FLYING",
        "vigilance": "VIGILANCE",
    }
    assert store_cards.keywords()["flying"] == "FLYING"


def test_a_creature_with_no_text_or_only_keywords_is_complete(
    known: dict[str, str],
) -> None:
    assert store_cards.complete(creature("Bear"), known) == []
    angel = creature("Angel", "Flying, vigilance")
    assert store_cards.complete(angel, known) == ["FLYING", "VIGILANCE"]
    drake = creature("Drake", "Flying (It can't be blocked.)\nFirst strike")
    assert store_cards.complete(drake, known) == ["FLYING", "FIRST_STRIKE"]


def test_anything_more_needs_a_file(known: dict[str, str]) -> None:
    for record in (
        creature("Trampler", "Trample"),
        creature("Drawer", "Flying\nWhen this creature enters, draw a card."),
        creature("Hybrid", cost="{1}{G/W}"),
        creature("Wall Art", type_line="Artifact"),
        {**creature("Bear Token"), "first_set": None},
        {**creature("Leveler"), "layout": "leveler"},
    ):
        assert store_cards.complete(record, known) is None, record["name"]


def test_path_says_a_complete_card_needs_no_file(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    store = tmp_path / "store.tsv"
    bear = creature("Grizzly Bears")
    card_store.write(store, [card_store.line("card", "Grizzly Bears", bear)])
    sources = card.Sources(store, tmp_path / "vocabulary", tmp_path / "cards")
    assert card.run_path(argparse.Namespace(name="Grizzly Bears"), sources) == 0
    printed = capsys.readouterr().out
    assert "Grizzly Bears is complete from the store, with no file" in printed
    assert "cards_1993_1994" in printed


def test_a_crate_is_three_files_that_differ_only_in_its_name() -> None:
    assert card_crates.years("cards_1993_1994") == "from 1993 to 1994"
    assert card_crates.years("cards_2022") == "in 2022"
    files = card_crates.files_of("cards_2022")
    assert 'name = "cards_2022"' in files["Cargo.toml"]
    assert "beryl_card_build::set_crate();" in files["build.rs"]
    assert files["lib.rs"].startswith("//! The cards first printed in 2022:")


def test_the_registry_lists_the_crates_it_is_given() -> None:
    manifest = (
        "[dependencies]\n"
        'beryl_card = { path = "../beryl_card" }\n'
        'cards_a = { path = "../../cards/cards_a" }\n'
        "\n[features]\nscenarios = [\n"
        '    "dep:beryl_scenario",\n'
        '    "cards_a/scenarios",\n'
        "]\n"
    )
    out = card_crates.registry(manifest, ["cards_a", "cards_b"])
    assert 'cards_b = { path = "../../cards/cards_b" }\n' in out
    assert '    "cards_a/scenarios",\n    "cards_b/scenarios",\n]' in out
    assert out.count("cards_a = ") == 1


def test_the_tree_holds_every_crate_current() -> None:
    """The `card-crates` check on the real tree."""
    assert card_crates.changes() == {}
    assert card_crates.extra() == []
    wanted = card_crates.wanted()
    assert wanted == [g.crate for g in set_groups.load() if g.crate in wanted]
