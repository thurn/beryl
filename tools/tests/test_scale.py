import re
from pathlib import Path

from beryl_tools import scale

TIME = """        3.21 real        10.00 user         1.00 sys
           987654321  maximum resident set size
                   0  average shared memory size
"""


def test_the_pool_is_real_cards_that_reach_only_names_and_tokens() -> None:
    pool = scale.sources()
    assert len(pool) > 20
    assert any(s.name == "Counterspell" for s in pool)
    for source in pool:
        assert set(scale.CRATE_PATH.findall(source.text)) <= {"names", "tokens"}


def test_a_clone_renames_only_its_own_card() -> None:
    counterspell = next(s for s in scale.sources() if s.name == "Counterspell")
    text = scale.clone(counterspell, 7)
    assert "name: names::COUNTERSPELL_7," in text
    assert "hand(P0, &[names::COUNTERSPELL_7])" in text
    assert not re.search(r"names::COUNTERSPELL\b", text)
    assert "names::MANA_CRYPT" in text  # the other cards stay what they are
    assert (
        "Behavior::Data(&counter(TARGET))" in text
    )  # its body and scenarios come along


def test_shard_counts_split_evenly_and_never_leave_one_empty() -> None:
    assert scale.shard_counts(10, 4) == [3, 3, 3, 1]
    assert scale.shard_counts(8, 4) == [2, 2, 2, 2]
    assert scale.shard_counts(2, 5) == [1, 1]
    assert sum(scale.shard_counts(50_000, 20)) == 50_000


def test_generated_workspace_holds_every_card_once(tmp_path: Path) -> None:
    pool = scale.sources()
    cards = len(pool) + 3  # the pool is cycled
    shards = scale.generate(tmp_path, cards, 2, pool)
    assert shards == ["shard0", "shard1"]
    names = []
    for shard in shards:
        lib = (tmp_path / shard / "src" / "lib.rs").read_text()
        names += re.findall(r'pub const \w+: &str = "(.*? \d+)";', lib)
        assert (
            'path = "' + str(scale.CRATES / "beryl_cards")
            in (tmp_path / shard / "Cargo.toml").read_text()
        )
    assert len(names) == cards == len(set(names))
    # A clone compiles only if its shard depends on every crate its card file uses.
    used = {
        crate
        for source in pool
        for crate in re.findall(r"^use (\w+)::", source.text, re.MULTILINE)
    } - {"crate", "super", "std"}
    manifest = (tmp_path / "shard0" / "Cargo.toml").read_text()
    assert used and all(f'{crate} = {{ path = "' in manifest for crate in used)
    assert len(list(tmp_path.glob("shard*/src/c*.rs"))) == cards
    main = (tmp_path / "all" / "src" / "main.rs").read_text()
    for shard in shards:
        # Every table is read through black_box, or the linker would drop the cards.
        assert f"black_box({shard}::CARDS)" in main
        assert f"black_box({shard}::SCENARIOS)" in main
    assert '"all", "shard0", "shard1"' in (tmp_path / "Cargo.toml").read_text()
    assert (tmp_path / "Cargo.lock").exists()


def test_regenerating_keeps_the_target_directory(tmp_path: Path) -> None:
    pool = scale.sources()
    scale.generate(tmp_path, 4, 2, pool)
    (tmp_path / "target").mkdir()
    (tmp_path / "target" / "built").write_text("x")
    scale.generate(tmp_path, 3, 1, pool)
    assert (tmp_path / "target" / "built").exists()
    assert not (tmp_path / "shard1").exists()


def test_time_report_parsing() -> None:
    assert scale.parse_time(TIME) == (3.21, 987654321)


def test_report_names_the_largest_process() -> None:
    runs = {
        "cold cargo check": scale.Run(2.0, 2.0, 2**30),
        "release build": scale.Run(9.0, 9.0, 3 * 2**30),
    }
    text = scale.report(2000, 4, 41, runs)
    assert "2,000 cards" in text
    assert "3.00 GiB   (release build)" in text


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
    return scale.Source(Path(f"{name}.rs"), "", name.upper(), name)


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
