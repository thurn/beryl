"""A card file's generated header: written from its card's record, rewritten in place with the
notes below it kept, and a stale one failing the `card-headers` step."""

import shutil
from pathlib import Path

import pytest

from beryl_tools import card_files, card_header, card_store, paths

JACE: card_store.Json = {
    "name": "Jace, Vryn's Prodigy // Jace, Telepath Unbound",
    "faces": [
        {
            "name": "Jace, Vryn's Prodigy",
            "mana_cost": "{1}{U}",
            "type_line": "Legendary Creature — Human Wizard",
            "oracle_text": "{T}: Draw a card, then discard a card. If there are five or more "
            "cards in your graveyard, exile Jace, then return him to the battlefield "
            "transformed under his owner's control.",
            "power": "0",
            "toughness": "2",
        },
        {
            "name": "Jace, Telepath Unbound",
            "mana_cost": "",
            "type_line": "Legendary Planeswalker — Jace",
            "oracle_text": "+1: Up to one target creature gets -2/-0.\n−9: You get an emblem.",
            "loyalty": "5",
        },
    ],
}


def test_each_face_is_printed_between_the_markers() -> None:
    assert card_header.header(JACE) == (
        f"{card_header.START}\n"
        "//! Jace, Vryn's Prodigy {1}{U}\n"
        "//! Legendary Creature — Human Wizard, 0/2\n"
        "//!\n"
        "//! {T}: Draw a card, then discard a card. If there are five or more cards in your "
        "graveyard, exile\n"
        "//! Jace, then return him to the battlefield transformed under his owner's control.\n"
        "//!\n"
        "//! Jace, Telepath Unbound\n"
        "//! Legendary Planeswalker — Jace, loyalty 5\n"
        "//!\n"
        "//! +1: Up to one target creature gets -2/-0.\n"
        "//! −9: You get an emblem.\n"
        f"{card_header.END}\n"
    )


def test_a_rewrite_replaces_the_header_and_keeps_the_notes_below_it() -> None:
    first = card_header.with_header("//! Jace. Typed by hand.\n\nuse x;\n", JACE)
    assert first == card_header.header(JACE) + "\nuse x;\n"
    noted = first.replace("\nuse x;", "//! A note.\n\nuse x;")
    changed = noted.replace("loyalty 5", "loyalty 4")
    assert card_header.with_header(changed, JACE) == noted
    assert card_header.with_header(noted, JACE) == noted


def test_a_stale_header_is_found(tmp_path: Path) -> None:
    real = paths.REPO / "engine/cards/cards_1993/lea/counterspell.rs"
    copy = tmp_path / "lea" / "counterspell.rs"
    copy.parent.mkdir()
    shutil.copy(real, copy)
    with card_store.Store() as store:
        assert card_header.stale([copy], store) == []
        copy.write_text(copy.read_text().replace("Counter target spell.", "Counter."))
        assert card_header.stale([copy], store) == [copy]


def test_the_step_passes_on_a_card_file_with_a_current_header(
    capsys: pytest.CaptureFixture[str],
) -> None:
    counterspell = "engine/cards/cards_1993/lea/counterspell.rs"
    assert card_header.step(counterspell) == 0
    assert card_files.FILE.match(counterspell)
    assert capsys.readouterr().out == ""
