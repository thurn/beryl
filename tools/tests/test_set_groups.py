"""data/set-groups.json and the paths of card files (set_groups.py)."""

from pathlib import Path

import pytest

from beryl_tools import card_store, set_groups
from beryl_tools.set_groups import Group, SetInfo


def committed_cards() -> list[card_store.Json]:
    with card_store.Store() as store:
        return [r for kind, r in store if kind == "card"]


def sizes(groups: list[Group], sets: dict[str, SetInfo]) -> list[int]:
    return [sum(sets[c].cards for c in g.sets if c in sets) for g in groups]


def test_the_first_run_partitions_sets_oldest_first_within_bounds() -> None:
    sets = {
        "new": SetInfo("2001-01-01", 400),
        "old": SetInfo("1993-08-05", 300),
        "mid": SetInfo("1995-01-01", 900),
        "huge": SetInfo("1999-01-01", 2000),
        "late": SetInfo("2002-01-01", 200),
    }
    groups = set_groups.extend([], sets)
    assert [g.sets for g in groups] == [["old", "mid"], ["huge"], ["new", "late"]]
    assert [g.crate for g in groups] == [
        "cards_1993_1995",
        "cards_1999",
        "cards_2001_2002",
    ]
    assert sizes(groups, sets) == [1200, 2000, 600]


def test_a_new_set_moves_no_existing_set() -> None:
    sets = {"a": SetInfo("2020-01-01", 1000), "b": SetInfo("2021-01-01", 900)}
    first = set_groups.extend([], sets)
    # A new set, and one Scryfall dates before every other, join the newest group or open one.
    sets |= {
        "c": SetInfo("2022-01-01", 500),
        "d": SetInfo("2023-01-01", 700),
        "early": SetInfo("1990-01-01", 50),
    }
    again = set_groups.extend(first, sets)
    assert [g.sets for g in first] == [["a"], ["b"]]
    assert [g.sets for g in again] == [["a"], ["b", "early", "c"], ["d"]]
    assert again[-1].crate == "cards_2023"


def test_regenerating_the_committed_groups_with_a_new_set_moves_none(
    tmp_path: Path,
) -> None:
    path = tmp_path / "set-groups.json"
    path.write_bytes(set_groups.GROUPS.read_bytes())
    before = set_groups.SetGroups(set_groups.load(path)).crates
    added: card_store.Json = {
        "first_set": "zzz",
        "first_released": "2030-01-01",
        "name": "New Card",
    }
    after = set_groups.SetGroups(
        set_groups.update(path, [*committed_cards(), added])
    ).crates
    assert {c: after[c] for c in before} == before
    assert after["zzz"] in {g.crate for g in set_groups.load(path)}


def test_the_committed_groups_cover_every_first_printing_set_within_bounds() -> None:
    records = committed_cards()
    sets = set_groups.first_sets(records)
    groups = set_groups.load()
    # `by cards` wrote them from this store: running it again changes nothing.
    assert set_groups.extend(groups, sets) == groups
    placed = [c for g in groups for c in g.sets]
    assert len(placed) == len(set(placed))
    assert set(sets) <= set(placed)
    crates = [g.crate for g in groups]
    assert len(crates) == len(set(crates))
    for group, size in zip(groups, sizes(groups, sets)):
        alone = len(group.sets) == 1 and size > set_groups.MAXIMUM
        assert size <= set_groups.MAXIMUM or alone, group.crate
    # Only the newest group may still be filling.
    assert min(sizes(groups[:-1], sets)) >= set_groups.MINIMUM


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
        "jace_vryn_s_prodigy_jace_telepath_unbound.rs",
    )
    with pytest.raises(LookupError, match="zzz"):
        groups.group_of({"first_set": "zzz", "name": "New Card"})


def test_modules_are_rust_identifiers() -> None:
    assert set_groups.set_module("m10") == "m10"
    assert set_groups.set_module("10e") == "set_10e"
    assert set_groups.set_module("mod") == "set_mod"
    assert set_groups.card_module("Lim-Dûl's Vault") == "lim_dul_s_vault"
    assert set_groups.card_module("Fire // Ice") == "fire_ice"
    assert set_groups.card_module("+2 Mace") == "card_2_mace"
    assert set_groups.card_module("Static") == "card_static"
    with pytest.raises(ValueError):
        set_groups.set_module("P30A")
