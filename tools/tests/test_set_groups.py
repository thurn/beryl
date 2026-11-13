"""data/set-groups.json and the paths of card files (set_groups.py)."""

from datetime import date
from pathlib import Path

import pytest

from beryl_tools import card_store, set_groups
from beryl_tools.set_groups import Group, SetInfo


def committed_cards() -> list[card_store.Json]:
    with card_store.Store() as store:
        return [r for kind, r in store if kind == "card"]


def sizes(groups: list[Group], sets: dict[str, SetInfo]) -> list[int]:
    return [sum(sets[c].cards for c in g.sets if c in sets) for g in groups]


TODAY = date(2026, 11, 12)


def test_sets_are_grouped_by_year_and_large_years_split() -> None:
    sets = {
        "new": SetInfo("2001-03-01", 400),
        "old": SetInfo("1993-08-05", 300),
        "mid": SetInfo("1993-01-01", 900),
        # 2010 holds 1,300 cards, so it is split into halves; its first half holds 1,250, so
        # that half is split into quarters.
        "jan": SetInfo("2010-01-01", 700),
        "may": SetInfo("2010-05-01", 550),
        "aug": SetInfo("2010-08-01", 50),
        # The current year and later ones are split into quarters however small.
        "now": SetInfo("2026-09-01", 10),
        "next": SetInfo("2027-02-01", 10),
    }
    groups = set_groups.extend([], sets, TODAY)
    assert [(g.crate, g.sets) for g in groups] == [
        ("cards_1993", ["mid", "old"]),
        ("cards_2001", ["new"]),
        ("cards_2010_q1", ["jan"]),
        ("cards_2010_q2", ["may"]),
        ("cards_2010_h2", ["aug"]),
        ("cards_2026_q3", ["now"]),
        ("cards_2027_q1", ["next"]),
    ]


def test_a_split_is_kept_and_a_set_moves_only_when_its_year_overflows() -> None:
    sets = {"a": SetInfo("2020-01-01", 600), "b": SetInfo("2021-08-01", 900)}
    first = set_groups.extend([], sets, TODAY)
    assert [g.crate for g in first] == ["cards_2020", "cards_2021"]
    # A new set that keeps its year under the cap joins it; one that takes it over splits it.
    sets |= {"c": SetInfo("2020-12-01", 500), "d": SetInfo("2021-02-01", 400)}
    again = set_groups.extend(first, sets, TODAY)
    assert [(g.crate, g.sets) for g in again] == [
        ("cards_2020", ["a", "c"]),
        ("cards_2021_h1", ["d"]),
        ("cards_2021_h2", ["b"]),
    ]
    # Once split, a year stays split though its sets shrink, and the current year stays split
    # in later years.
    smaller = {"b": sets["b"], "now": SetInfo("2026-01-01", 10)}
    split = set_groups.extend(set_groups.extend([], smaller, TODAY), smaller, TODAY)
    later = set_groups.extend([*again, *split], smaller, date(2030, 1, 1))
    assert [g.crate for g in later] == ["cards_2021_h2", "cards_2026_q1"]


def test_the_committed_groups_follow_the_rule_and_cover_every_set() -> None:
    records = committed_cards()
    sets = set_groups.first_sets(records)
    groups = set_groups.load()
    # `card crates` wrote them from this store: running it again changes nothing.
    assert set_groups.extend(groups, sets) == groups
    placed = [c for g in groups for c in g.sets]
    assert len(placed) == len(set(placed))
    assert set(sets) == set(placed)
    for group, size in zip(groups, sizes(groups, sets)):
        assert set_groups.CRATE.match(group.crate), group.crate
        quarter = group.crate[-2] == "q"
        assert size <= set_groups.CAP or quarter, group.crate
        assert all(sets[c].released[:4] in group.crate for c in group.sets)


def test_every_card_has_a_path_of_its_own() -> None:
    records = committed_cards()
    groups = set_groups.SetGroups()
    found = {groups.card_path(r): r["name"] for r in records}
    assert len(found) == len(records)
    jace = next(r for r in records if str(r["name"]).startswith("Jace, Vryn's Prodigy"))
    assert groups.card_path(jace, Path("cards")) == Path(
        "cards",
        groups.group_of(jace),
        "ori",
        "jace_vryns_prodigy.rs",
    )
    # A card of two faces is filed by its front face, the name the engine knows; a split card
    # by both halves.
    split: set_groups.Json = {
        "name": "Fire // Ice",
        "first_set": "apc",
        "layout": "split",
        "faces": [{"name": "Fire"}, {"name": "Ice"}],
    }
    assert groups.card_path(split).name == "fire_ice.rs"
    assert set_groups.constant("Jace, Vryn's Prodigy") == "JACE_VRYNS_PRODIGY"
    with pytest.raises(LookupError, match="zzz"):
        groups.group_of({"first_set": "zzz", "name": "New Card"})


def test_modules_are_rust_identifiers() -> None:
    assert set_groups.set_module("m10") == "m10"
    assert set_groups.set_module("10e") == "set_10e"
    assert set_groups.set_module("mod") == "set_mod"
    assert set_groups.card_module("Lim-Dûl's Vault") == "lim_duls_vault"
    assert set_groups.card_module("Fire // Ice") == "fire_ice"
    assert set_groups.card_module("+2 Mace") == "card_2_mace"
    assert set_groups.card_module("Static") == "card_static"
    with pytest.raises(ValueError):
        set_groups.set_module("P30A")
