"""Beryl 3D's families for Ignite: what each check reads, how each tool's output becomes one
result per check, and that the adapter imports none of the orchestration the switch deletes.
Running the programs is left to the families' own runs, which need the `b3d` build."""

from __future__ import annotations

import json
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from beryl_tools import paths
from beryl_tools.beryl3d import gate, timeline
from beryl_tools.ignite import Result
from beryl_tools.ignite import beryl3d as adapter

MANIFEST: Path = paths.REPO / "ignite" / "checks" / "beryl3d.toml"

FORBIDDEN: list[str] = str(
    "tokens lanes step_runner selection rust_fit python_fit runlog runstore verdicts budget "
    "hang flaky bisect slow_ci sweep nightly warm caches incidents deadline clock check_value "
    "checks"
).split()


def manifest() -> dict[str, dict[str, dict[str, object]]]:
    return tomllib.loads(MANIFEST.read_text())


def test_the_adapter_imports_no_orchestration() -> None:
    code = (
        "import sys, beryl_tools.ignite.beryl3d; "
        "print(' '.join(m for m in sys.modules if m.startswith('beryl_tools.')))"
    )
    loaded = subprocess.run(
        [sys.executable, "-c", code], capture_output=True, text=True, check=True
    ).stdout.split()
    assert not [m for m in loaded if m.removeprefix("beryl_tools.") in FORBIDDEN]


def test_each_jsonl_family_is_listed_and_run_by_its_own_name() -> None:
    for name, family in manifest()["family"].items():
        if family["adapter"] != "jsonl":
            continue
        assert name in adapter.LISTS and name in adapter.RUNS, name
        assert family["list"] == (
            f"uv run --quiet --project tools python -m beryl_tools.ignite.beryl3d {name} list"
        )
        assert str(family["run"]).endswith(f"beryl3d {name} run {{checks}}")


def test_the_programs_that_reread_their_grant_ask_for_one_that_grows() -> None:
    """`record` (the end of the `b3d` build) and `b3d test` start more threads when the file
    `IGNITE_CORES_FILE` names rises, so they ask for grants that grow; the others do not.
    """
    loaded = manifest()
    assert loaded["product"]["b3d"].get("grows") is True
    growing = {n for n, f in loaded["family"].items() if f.get("grows")}
    assert growing == {"b3d-interface", "b3d-every-tick"}


def test_the_families_that_read_recordings_name_every_crate_the_recorder_is_built_from() -> (
    None
):
    """Beryl 3D reaches the main workspace by path, which `crate:` does not follow, so these
    families name the main crates themselves. A change to `beryl_session` must reach them.
    """
    crates = adapter.main_crates()
    assert "rust/crates/beryl_session/**" in crates
    for name in (
        "b3d-interface",
        "b3d-every-tick",
        "b3d-gate",
        "b3d-timeline",
        "b3d-frame-counts",
    ):
        inputs = manifest()["family"][name]["inputs"]
        assert isinstance(inputs, list)
        named = [i for i in inputs if str(i).startswith("rust/crates/")]
        assert named == crates, name
        # A card file is each check's own input, never the family's.
        assert not [
            i for i in inputs if str(i).startswith("rust/cards/") and "*/" not in i
        ]


def test_a_recording_reads_the_cards_either_seat_saw_and_no_other() -> None:
    names = {
        "Counterspell": "rust/cards/a/lea/counterspell.rs",
        "Island": "rust/cards/a/lea/island.rs",
        "Fable of the Mirror-Breaker": "rust/cards/b/neo/fable_of_the_mirror_breaker.rs",
        "Double Major": "rust/cards/c/stx/double_major.rs",
    }
    exported = {
        "counterspell/cast": [
            '{"objects":[{"name":"Counterspell"}]}',
            '{"hand":[{"name":"Island"},{"name":"Counterspell"}]}',
        ],
        "verbs-zones": ['{"text":"no card here"}'],
    }
    found = adapter.recording_cards(exported, names, {})
    assert found == {
        "counterspell/cast": [
            "rust/cards/a/lea/counterspell.rs",
            "rust/cards/a/lea/island.rs",
        ],
        "verbs-zones": [],
    }
    assert not any("double_major" in c for cards in found.values() for c in cards)


def test_an_agents_fixture_reads_its_whole_deck_and_a_fuzz_deck_every_card() -> None:
    files = ["rust/cards/a/lea/counterspell.rs", "rust/cards/a/lea/island.rs"]
    defined: list[dict[str, object]] = [
        {"name": "opening-play", "decks": ["wu-control", "wu-control"]},
        {"name": "fuzzed", "decks": ["fuzz:3", "wu-control"]},
        {"name": "table-opening", "table": {"deck": "small.txt"}},
        {"name": "game-over", "board": "game-over"},
    ]
    decks = {"wu-control": "Deck\n4 Counterspell\n", "small.txt": "2 Island\n"}
    found = adapter.fixture_decks(defined, decks, files)
    assert found == {
        "opening-play": {"rust/cards/a/lea/counterspell.rs"},
        "fuzzed": set(files),
        "table-opening": {"rust/cards/a/lea/island.rs"},
    }
    cards = adapter.recording_cards({"opening-play": ["{}"]}, {}, found)
    assert cards == {"opening-play": ["rust/cards/a/lea/counterspell.rs"]}


def test_interface_results_follow_each_tests_outcome() -> None:
    output: dict[str, object] = {
        "results": [
            {"test": "b3d:a/you/1440x900", "outcome": "pass"},
            {"test": "b3d:b/you/1440x900", "outcome": "fail"},
        ],
        "faults": [{"test": "b3d:b/you/1440x900", "rule": "reachable"}],
        "over_limits": ["a/you/1440x900 drew 900 frames, over `most_frames` 800"],
    }
    wanted = ["a/you/1440x900", "b/you/1440x900", "c/you/1440x900", "frame-limits"]
    results = adapter.interface_results(output, "b3d-interface", wanted)
    outcomes = {r.check: r.outcome for r in results}
    assert outcomes == {
        "b3d-interface:a/you/1440x900": "pass",
        "b3d-interface:b/you/1440x900": "fail",
        "b3d-interface:frame-limits": "fail",
    }
    assert "reachable" in results[1].message


def test_a_run_of_some_tests_plays_only_their_common_prefix() -> None:
    assert adapter.common_prefix(["a/b/you/1", "a/b/opponent/1"]) == "a/b/"
    assert adapter.common_prefix(["a/you/1", "b/you/1"]) == ""


def test_a_batch_across_recordings_runs_only_its_recordings(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    universe: list[str] = [
        f"{r}/{s}/1440x900" for r in "abc" for s in ("you", "opponent")
    ]
    played: list[str] = []

    def fake(
        command: list[str], cwd: Path = adapter.WORKSPACE
    ) -> subprocess.CompletedProcess[str]:
        prefix = command[2] if command[2] != "--json" else ""
        names = [t for t in universe if t.startswith(prefix)]
        played.extend(names)
        rows = [{"test": f"b3d:{t}", "outcome": "pass"} for t in names]
        return subprocess.CompletedProcess(
            command, 0, json.dumps({"results": rows}), ""
        )

    monkeypatch.setattr(adapter, "run_out", fake)
    results = adapter.interface_run(
        "b3d-interface",
        ["b3d-interface:a/you/1440x900", "b3d-interface:c/opponent/1440x900"],
        [],
    )
    assert sorted(played) == ["a/you/1440x900", "c/opponent/1440x900"]
    assert [(r.check, r.outcome) for r in results] == [
        ("b3d-interface:a/you/1440x900", "pass"),
        ("b3d-interface:c/opponent/1440x900", "pass"),
    ]


def frame(index: int, **counts: float) -> dict[str, object]:
    keys = [*gate.COUNTERS, *gate.LOAD, "frame_allocations"]
    every = {c: 1.0 for c in keys}
    return {"frame": index, "ms": index * 16, "phase": "rest", **every, **counts}


def test_gate_results_hold_each_board_to_its_own_limits() -> None:
    output = json.dumps(
        {
            "boards": [
                {
                    "name": name,
                    "frames": [frame(0), frame(1, elements=3.0)],
                    "handle_allocations": 0,
                    "first_frame_allocation": None,
                }
                for name in ("held", "over")
            ],
            "enhanced": [],
        }
    )
    board = gate.parse(output)[0]
    exact = {m.key: gate.number(m.key, m.value) for m in gate.measure(board)}
    data: dict[str, object] = {
        "frames": {"held": exact, "over": {**exact, "worst_frame_elements": 2}}
    }
    results = adapter.gate_results(output, ["held", "over", "absent"], data)
    assert [(r.check, r.outcome) for r in results] == [
        ("b3d-gate:held", "pass"),
        ("b3d-gate:over", "fail"),
    ]
    assert "over its limit 2" in results[1].message


def step(start: float) -> dict[str, object]:
    return {
        "view": 1,
        "beat": 0,
        "step": 0,
        "verb": "fly",
        "case": "",
        "effects": [],
        "sounds": [],
        "own": False,
        "start": start,
        "end": start + 100,
    }


def test_timeline_results_name_each_fixture_over_its_allowance() -> None:
    web = timeline.parse(
        [json.dumps({"recording": r, "seat": s, **step(0)}) for r, s in KEYS]
    )
    native = timeline.parse(
        [json.dumps({"recording": r, "seat": s, **step(50)}) for r, s in KEYS]
    )
    scope = {timeline.key_of(r, s) for r, s in KEYS}
    found = timeline.differences(web, native, scope)
    allowed = {timeline.key_of("allowed", "p0"): 1}
    results = adapter.timeline_results(
        found, KEYS, allowed, ["allowed", "differs", "missing"]
    )
    assert [(r.check, r.outcome) for r in results] == [
        ("b3d-timeline:allowed", "pass"),
        ("b3d-timeline:differs", "fail"),
        ("b3d-timeline:missing", "fail"),
    ]
    assert "differs/p0" in results[1].message


KEYS: list[tuple[str, str]] = [("allowed", "p0"), ("differs", "p0")]


def test_format_results_blame_the_package_that_holds_the_file() -> None:
    root = paths.REPO / "rust" / "beryl3d" / "crates"
    output = f"Diff in {root}/beryl_scene/src/lib.rs:12:\n-a\n+b\n"
    dirs = {
        "beryl_scene": "rust/beryl3d/crates/beryl_scene",
        "beryl_inspect": "rust/beryl3d/crates/beryl_inspect",
    }
    results = adapter.format_results(1, output, dirs, ["beryl_scene", "beryl_inspect"])
    assert results == [
        Result(
            "b3d-format:beryl_scene",
            "fail",
            "not formatted (run `just fmt`):\nrust/beryl3d/crates/beryl_scene/src/lib.rs",
            "rust/beryl3d/crates/beryl_scene/src/lib.rs:12",
        ),
        Result("b3d-format:beryl_inspect", "pass"),
    ]
