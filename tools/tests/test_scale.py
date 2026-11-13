import json
import re
from pathlib import Path

from beryl_tools import card_store, scale, scale_workspace

TIME = """        3.21 real        10.00 user         1.00 sys
           987654321  maximum resident set size
                   0  average shared memory size
"""


def test_every_card_file_is_in_the_pool() -> None:
    pool, left = scale_workspace.sources()
    assert len(pool) > 40 and not left
    assert any(s.name == "Counterspell" for s in pool)


def test_a_clone_renames_only_its_own_card_and_faces() -> None:
    pool, _ = scale_workspace.sources()
    counterspell = next(s for s in pool if s.name == "Counterspell")
    text = scale_workspace.clone(counterspell, 7)
    assert "names::COUNTERSPELL_7" in text
    assert "printed::COUNTERSPELL_7" in text
    assert not re.search(r"(names|printed)::COUNTERSPELL\b", text)
    assert "props::ARTIFACT" in text  # the other cards stay what they are
    assert ".effect(&counter(TARGET))" in text  # its body and scenarios come along
    record = scale_workspace.clone_record(counterspell, 7, "z00001")
    assert record["name"] == "Counterspell 7" and record["first_set"] == "z00001"


def test_a_card_of_two_faces_renames_each_face() -> None:
    pool, _ = scale_workspace.sources()
    several = next(s for s in pool if len(s.faces()) > 1)
    renamed = scale_workspace.renames(several, 3)
    faces = [scale_workspace.set_groups.constant(f) for f in several.faces()]
    assert all(renamed[face] == f"{face}_3" for face in faces)
    assert scale_workspace.clone_name(several, 3).endswith(" 3")


def test_the_plan_follows_the_trees_groups() -> None:
    groups = [(5, 1000), (2, 500)]
    crates = scale_workspace.plan(3000, None, groups)
    # The mean group is 750 cards: 4 crates, sized 2:1 as the groups cycle.
    assert [c.cards for c in crates] == [1000, 500, 1000, 500]
    assert [len(c.sets) for c in crates] == [5, 2, 5, 2]
    assert sum(c.cards for c in scale_workspace.plan(7, 3, groups)) == 7
    assert all(n for c in scale_workspace.plan(3, 9, groups) for _, n in c.sets)


def test_shares_are_exact() -> None:
    assert scale_workspace.shares(10, [1, 1, 1]) == [4, 3, 3]
    assert sum(scale_workspace.shares(50_000, [7, 3, 11])) == 50_000


def test_the_workspace_is_the_trees_structure(tmp_path: Path) -> None:
    pool, _ = scale_workspace.sources()
    crates = scale_workspace.plan(len(pool) + 3, 2, [(2, 1)])
    names = scale_workspace.generate(tmp_path, crates, pool)
    assert names == ["cards_s000", "cards_s001"]
    files = sorted((tmp_path / "rust" / "cards").glob("*/*/*.rs"))
    assert len(files) == len(pool) + 3
    # Each clone has a record in the scratch store, first printed in its directory's set.
    with card_store.Store(tmp_path / "data" / "card-store.tsv") as store:
        for path in files:
            name = scale_workspace.card_files.name_of(path, store)
            assert name is not None, path
    # The set crates are the tree's but for their paths to the checkout.
    crate = tmp_path / "rust" / "cards" / "cards_s000"
    assert (crate / "build.rs").read_text() == (scale_workspace.card_crates.BUILD)
    assert (
        str(scale_workspace.CRATES / "beryl_card_build")
        in (crate / "Cargo.toml").read_text()
    )
    registry = (tmp_path / "rust" / "crates" / "beryl_cards" / "Cargo.toml").read_text()
    for name in names:
        assert f"\n{name} = " in registry and f'"{name}/scenarios"' in registry
    groups = json.loads((tmp_path / "data" / "set-groups.json").read_text())
    assert [g["crate"] for g in groups["groups"]] == names
    workspace = (tmp_path / "rust" / "Cargo.toml").read_text()
    assert 'members = ["cards/*", "crates/beryl_cards", "crates/warm"]' in workspace
    assert "[profile.release]" in workspace
    for copied in scale_workspace.COPIED:
        assert (tmp_path / copied).is_file()


def test_regenerating_keeps_the_target_directory(tmp_path: Path) -> None:
    pool, _ = scale_workspace.sources()
    scale_workspace.generate(tmp_path, scale_workspace.plan(4, 2, [(1, 1)]), pool)
    (tmp_path / "target").mkdir()
    (tmp_path / "target" / "built").write_text("x")
    scale_workspace.generate(tmp_path, scale_workspace.plan(3, 1, [(1, 1)]), pool)
    assert (tmp_path / "target" / "built").exists()
    assert not (tmp_path / "rust" / "cards" / "cards_s001").exists()


def test_time_report_parsing() -> None:
    assert scale.parse_time(TIME) == (3.21, 11.0, 987654321)


def runs(check: float) -> dict[str, scale.Run]:
    return {
        "cold check": scale.Run(check, check, 100.0, 2**30, 4.0),
        "release build": scale.Run(90.0, 90.0, 300.0, 3 * 2**30, 4.0),
        "one-edit rebuild": scale.Run(1.0, 1.0, 1.0, 2**29, 4.0),
    }


def test_the_report_gives_each_budget_and_margin() -> None:
    text = scale.report(2000, 4, False, 41, runs(45.0))
    assert "2,000 cards" in text and "as the tree's grouping gives" in text
    assert "45.0 s   budget 60.0 s, margin 15.0 s  ok" in text
    assert "3.00 GiB   budget 4.00 GiB, margin 1.00 GiB  ok" in text
    assert "100 s CPU" in text


def test_a_measure_over_its_budget_is_missed() -> None:
    assert scale.missed(scale.measures(runs(45.0))) == []
    assert scale.missed(scale.measures(runs(61.0))) == ["cold check"]
    assert "MISSED" in scale.report(2000, 4, False, 41, runs(61.0))


def test_the_nightly_holds_only_memory_which_load_does_not_move() -> None:
    slow = runs(61.0)
    assert scale.missed(scale.measures(slow), times=False) == []
    slow["release build"].rss = 5 * 2**30
    assert scale.missed(scale.measures(slow), times=False) == [
        "largest compiler process"
    ]


def test_card_seconds_sum_each_cards_checks_and_skip_recordings() -> None:
    lines = [
        '{"check": "script:counterspell/start/yours", "outcome": "pass", "seconds": 0.5}',
        '{"check": "script:counterspell/start/theirs", "outcome": "pass", "seconds": 0.25}',
        '{"check": "script:brainstorm/cast/yours", "outcome": "pass", "seconds": 1.0}',
        '{"check": "script:fixtures/stack-two/yours", "outcome": "pass", "seconds": 9.0}',
        "not a record",
    ]
    cards, count = scale.card_seconds(lines)
    assert cards == {"counterspell": 0.75, "brainstorm": 1.0}
    assert count == 3


def source(name: str) -> scale.Source:
    return scale.Source(Path(f"{name}.rs"), "", name, {"faces": [{"name": name}]})


def test_clones_cost_what_their_originals_do_cycled_as_generated() -> None:
    pool = [source("Counterspell"), source("Brainstorm"), source("Ponder")]
    cards = {"counterspell": 2.0, "brainstorm": 1.0}
    # Clone i is pool[i % 3]: 7 clones are two rounds of 3.0 s and one Counterspell.
    assert scale.clone_seconds(cards, pool, 7) == 8.0
    assert scale.clone_seconds(cards, pool, 2) == 3.0


def test_the_script_report_extrapolates_at_the_runs_parallelism() -> None:
    pool = [source("Counterspell"), source("Brainstorm")]
    scripts = scale.Scripts(
        {"counterspell": 3.0, "brainstorm": 1.0},
        checks=6,
        seconds=4.0,
        wall=4.0,
        checking=2.0,
    )
    text = scale.script_report(scripts, pool, 1000)
    assert "6 checks" in text and "2 cards" in text
    assert "2.000 s" in text  # The mean of 3 and 1.
    assert "counterspell 3.00 s, brainstorm 1.00 s" in text
    # 500 rounds of 4 s, overlapping twofold as the run's 4 s of checks took 2 s.
    assert "2,000 s" in text and "about 1,000 s" in text and "(2.0x)" in text


def test_the_rebuild_is_the_median_run() -> None:
    made = [scale.Run(s, s, s, 0, 0.0) for s in (5.0, 3.0, 9.0)]
    assert scale.median(made).seconds == 5.0
