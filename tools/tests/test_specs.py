"""specs.py's rules text and nearest-card ranking, read from the card store with no network."""

from beryl_tools import specs


def test_a_decks_card_is_found_by_its_slug() -> None:
    text = specs.oracle("counterspell")
    assert text is not None and "Counter target spell." in text


def test_a_card_outside_the_decks_is_found_by_name_in_the_store() -> None:
    text = specs.oracle("Lightning Bolt")
    assert text is not None and "3 damage" in text


def test_nearest_ranks_only_the_specced_cards() -> None:
    near = specs.nearest("Lightning Bolt", {"counterspell", "mana-tithe"})
    assert set(near) <= {"Counterspell", "Mana Tithe"}
    assert specs.nearest("Counterspell", {"counterspell"}) == []
