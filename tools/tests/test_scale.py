import argparse
import json
import re
import subprocess
import sys
from dataclasses import replace
from pathlib import Path

import pytest

from beryl_tools import card_store, scale, scale_workspace

TIME = """        3.21 real        10.00 user         1.00 sys
           987654321  maximum resident set size
                   0  average shared memory size
"""


def test_every_card_file_is_in_the_pool() -> None:
    pool, left = scale_workspace.sources()
    assert len(pool) > 40 and not left
    assert any(s.name == "Counterspell" for s in pool)


def test_three_quarters_of_the_clones_have_no_scenarios_module() -> None:
    pool, _ = scale_workspace.sources()
    assert any(s.has_scenarios for s in pool) and any(not s.has_scenarios for s in pool)
    picked = [scale_workspace.original(pool, i) for i in range(4_000)]
    simple = sum(not s.has_scenarios for s in picked)
    assert simple == 3_000
    # Each kind is cycled through in order, so every file of both kinds is cloned.
    assert {s.path for s in picked} == {s.path for s in pool}
    only = [s for s in pool if s.has_scenarios]
    assert {scale_workspace.original(only, i).path for i in range(9)} <= {
        s.path for s in only
    }


def test_a_clone_renames_only_its_own_card_and_faces() -> None:
    pool, _ = scale_workspace.sources()
    counterspell = next(s for s in pool if s.name == "Counterspell")
    # A card can have no scenarios, so supply the name references the clone must rewrite.
    source = replace(
        counterspell,
        text=counterspell.text
        + "\nconst CARDS: &[&str] = &[Card::Counterspell, Card::Plains];\n",
    )
    text = scale_workspace.clone(source, 7)
    assert "Card::Counterspell7" in text
    assert "Printed::Counterspell7" in text
    assert not re.search(r"(Card|Printed)::Counterspell\b", text)
    assert "Card::Plains]" in text  # the other card keeps its name
    assert ".effect(&counter(target()))" in text
    record = scale_workspace.clone_record(counterspell, 7, "z00001")
    assert record["name"] == "Counterspell 7" and record["first_set"] == "z00001"


def test_a_card_of_two_faces_renames_each_face_and_its_shape() -> None:
    pool, _ = scale_workspace.sources()
    several = next(s for s in pool if len(s.faces()) > 1 and "PrintedShape::" in s.text)
    renamed = scale_workspace.renames(several, 3)
    faces = [scale_workspace.set_groups.constant(f) for f in several.faces()]
    assert all(renamed[face] == f"{face}_3" for face in faces)
    assert scale_workspace.clone_name(several, 3).endswith(" 3")
    shape = re.findall(r"PrintedShape::(\w+)", several.text)
    assert len(shape) == 1
    cloned = scale_workspace.clone(several, 3)
    assert f"PrintedShape::{shape[0]}3" in cloned


def test_the_scratch_store_keeps_the_cards_read_through_the_card_type() -> None:
    # A clone's scenario reads `Card::Plains`: without Plains in the scratch store, every
    # set crate's build script failed with "no card in the store is spelled so".
    pool, _ = scale_workspace.sources()
    readers = [
        s for s in pool if "Card::Plains" in s.text and "names::PLAINS" not in s.text
    ]
    assert readers
    keys = {
        line.partition(b"\t")[0] for line in scale_workspace.store_lines(readers[:1])
    }
    assert b"card:Plains" in keys


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
    files = sorted((tmp_path / "engine" / "cards").glob("*/*/*.rs"))
    assert len(files) == len(pool) + 3
    # Each clone has a record in the scratch store, first printed in its directory's set.
    with card_store.Store(tmp_path / "data" / "card-store.tsv") as store:
        for path in files:
            name = scale_workspace.card_files.name_of(path, store)
            assert name is not None, path
    # The set crates are the tree's but for their paths to the checkout.
    crate = tmp_path / "engine" / "cards" / "cards_s000"
    assert (crate / "build.rs").read_text() == (scale_workspace.card_crates.BUILD)
    assert (scale_workspace.CRATES / "beryl_card_build").as_posix() in (
        crate / "Cargo.toml"
    ).read_text()
    registry = (
        tmp_path / "engine" / "crates" / "beryl_cards" / "Cargo.toml"
    ).read_text()
    for name in names:
        assert f"\n{name} = " in registry and f'"{name}/scenarios"' in registry
    groups = json.loads((tmp_path / "data" / "set-groups.json").read_text())
    assert [g["crate"] for g in groups["groups"]] == names
    workspace = (tmp_path / "engine" / "Cargo.toml").read_text()
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
    assert not (tmp_path / "engine" / "cards" / "cards_s001").exists()


def test_time_report_parsing() -> None:
    assert scale.parse_time(TIME) == (3.21, 11.0, 987654321)


def test_windows_timing_includes_descendant_cpu_and_peak_memory(tmp_path: Path) -> None:
    if not scale.system.WINDOWS:
        pytest.skip("requires Win32 Job Objects")
    (tmp_path / "engine").mkdir()
    grandchild = """\
import time
data = bytearray(64 * 1024 * 1024)
deadline = time.process_time() + 0.2
while time.process_time() < deadline:
    pass
"""
    child = (
        "import subprocess, sys; "
        f"subprocess.run([sys.executable, '-c', {grandchild!r}], check=True)"
    )
    measured = scale.timed([sys.executable, "-c", child], tmp_path)
    assert measured.seconds > 0
    assert measured.cpu >= 0.2
    assert measured.peak_memory >= 64 * 1024 * 1024


def test_windows_timing_reports_a_failed_builds_stderr(tmp_path: Path) -> None:
    if not scale.system.WINDOWS:
        pytest.skip("requires Win32 Job Objects")
    (tmp_path / "engine").mkdir()
    # Check the child's own working directory and environment via its diagnostic.
    child = (
        "import os, sys; "
        "sys.exit(os.path.basename(os.getcwd()) + ' ' + os.environ['CARGO_TARGET_DIR'])"
    )
    with pytest.raises(SystemExit) as error:
        scale.timed([sys.executable, "-c", child], tmp_path)
    assert str(error.value).splitlines()[-1] == f"engine {tmp_path / 'target'}"


def runs(check: float) -> dict[str, scale.Run]:
    return {
        "cold check": scale.Run(check, 100.0, 2**30),
        "release build": scale.Run(90.0, 300.0, 3 * 2**30),
        "one-edit rebuild": scale.Run(1.0, 1.0, 2**29),
    }


def test_the_report_gives_each_budget_and_margin() -> None:
    limit = scale.BUDGETS["cold check"].limit
    text = scale.report(2000, 4, False, 41, runs(limit - 15.0), 1_234, 5_678)
    assert "2,000 cards" in text and "as the tree's grouping gives" in text
    assert f"{limit - 15.0:.1f} s   budget 60.0 s" in text
    assert "margin 15.0 s  ok" in text
    assert "3.00 GiB   budget 4.00 GiB, margin 1.00 GiB  ok" in text
    assert "100 s CPU" in text


def test_a_measure_over_its_budget_is_missed() -> None:
    limit = scale.BUDGETS["cold check"].limit
    assert scale.missed(scale.measures(runs(limit - 1.0))) == []
    assert scale.missed(scale.measures(runs(limit + 1.0))) == ["cold check"]
    over = runs(limit + 1.0)
    assert "MISSED" in scale.report(2000, 4, False, 41, over, 1_234, 5_678)


def test_scale_reports_target_bytes_before_and_after_edits_under_assert(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    target = tmp_path / "scale" / "target"
    release: Path = target / "release" / "all"
    incremental: Path = target / "debug" / "incremental"

    def clean(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        assert command[:2] == ["cargo", "clean"]
        return subprocess.CompletedProcess[str](command, 0)

    def timed_cargo(command: list[str], _root: Path) -> scale.Run:
        if "--bin=all" in command:
            release.parent.mkdir(parents=True)
            release.write_bytes(b"release")
        if "--features=scenarios" in command:
            incremental.mkdir(parents=True, exist_ok=True)
            artifact = incremental / "objects"
            with artifact.open("ab") as file:
                file.write(b"debug")
        return scale.Run(1.21, 11.0, 987654321)

    monkeypatch.setattr(scale.subprocess, "run", clean)
    monkeypatch.setattr(scale, "timed", timed_cargo)
    parser = argparse.ArgumentParser()
    scale.register(parser.add_subparsers())
    args = parser.parse_args("scale --cards 3 --edits 2 --no-scripts --assert".split())

    assert scale.scale(args) == 0
    captured = capsys.readouterr()
    assert re.search(r"^target after build\s+12 B$", captured.out, re.MULTILINE)
    assert re.search(r"^target after edits\s+22 B$", captured.out, re.MULTILINE)
    saved = (target.parent / "build-report.txt").read_text(encoding="utf-8")
    assert captured.out.endswith(saved)


def test_the_memory_assertion_ignores_times() -> None:
    slow = runs(61.0)
    assert scale.missed(scale.measures(slow), times=False) == []
    slow["release build"].peak_memory = 5 * 2**30
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
    made = [scale.Run(s, s, 0) for s in (5.0, 3.0, 9.0)]
    assert scale.median(made).seconds == 5.0
