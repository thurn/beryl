"""The Forge census on synthetic scripts: kind extraction, blocking order and the coverage
file's entries against the real vocabulary."""

from pathlib import Path

from beryl_tools import census, vocabulary

BOLT = """Name:Test Bolt
ManaCost:R
Types:Instant
A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SubAbility$ Fin
SVar:Fin:DB$ Cleanup | ClearRemembered$ True
SVar:X:Count$Valid Creature
Oracle:Text.
"""

CREATURE = """Name:Front Face
Types:Creature Wizard
K:Flying
K:Awaken:3:4 U
T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | Execute$ Go
S:Mode$ Continuous | Affected$ Creature
R:Event$ Moved | ActiveZones$ Graveyard
AlternateMode:DoubleFaced

ALTERNATE

Name:Back Face
K:Trample
"""


def test_kinds_of_reads_only_kind_names() -> None:
    names, found = census.kinds_of(BOLT)
    assert names == ["Test Bolt"]
    assert found == {"effect:DealDamage", "effect:Cleanup"}


def test_kinds_of_covers_every_category_and_both_faces() -> None:
    names, found = census.kinds_of(CREATURE)
    assert names == ["Front Face", "Back Face"]
    assert found == {
        "keyword:Flying",
        "keyword:Awaken",
        "keyword:Trample",
        "trigger:ChangesZone",
        "static:Continuous",
        "replacement:Moved",
    }


def test_report_orders_by_cards_only_that_kind_blocks() -> None:
    cards = [{"a", "b"}, {"a"}, {"a"}, {"b", "c"}, {"c"}, {"covered"}]
    r = census.report(cards, {"covered": "Effect::Draw"})
    assert r.cards == 6
    assert r.covered_cards == 1
    assert r.blocked_by == [("a", 2, 3), ("c", 1, 2), ("b", 0, 2)]
    assert r.counts["a"] == 3


def test_pool_kinds_keeps_only_pool_cards(tmp_path: Path) -> None:
    (tmp_path / "t").mkdir()
    (tmp_path / "t" / "bolt.txt").write_text(BOLT)
    (tmp_path / "t" / "other.txt").write_text("Name:Not In Pool\nK:Flying\n")
    (tmp_path / "t" / "dfc.txt").write_text(CREATURE)
    kept = census.pool_kinds(tmp_path, {"Test Bolt", "Back Face"})
    assert len(kept) == 2


def test_stale_entries_are_reported() -> None:
    assert census.stale(
        {"effect:A": "Effect::Draw", "effect:B": "Effect::Gone"}, ["Effect::Draw"]
    ) == ["effect:B"]


def test_coverage_file_names_real_vocabulary_entries() -> None:
    covered = census.coverage()
    assert covered
    assert census.stale(covered, (e.name for e in vocabulary.entries())) == []
