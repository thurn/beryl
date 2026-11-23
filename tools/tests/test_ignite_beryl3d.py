"""Beryl 3D's families for Ignite: what each check reads, how each tool's output becomes one
result per check, and that the adapter imports none of the orchestration the switch deletes.
Running the programs is left to the families' own runs, which need the `b3d` build."""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from beryl_tools import calibration, paths
from beryl_tools.beryl3d import frame_profiles, gate, pack, timeline
from beryl_tools.beryl3d.match import reference, score
from beryl_tools.beryl3d.match.target import Target
from beryl_tools.ignite import RESULTS_ENV, Result
from beryl_tools.ignite import beryl3d as adapter
from beryl_tools.ignite import beryl3d_match as web_match

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
        if name == "b3d-corpus":
            # The corpus streams results through its own command dispatch.
            assert str(family["next"]).endswith(f"beryl3d {name} next")
        else:
            assert name in adapter.LISTS and name in adapter.RUNS, name
        assert family["list"] == (
            f"uv run --quiet --project tools python -m beryl_tools.ignite.beryl3d {name} list"
        )
        assert str(family["run"]).endswith(f"beryl3d {name} run {{checks}}")


def test_the_programs_that_reread_their_grant_ask_for_one_that_grows() -> None:
    """The recorder, replay checks and Nextest's parallel replay tests follow growing grants."""
    loaded = manifest()
    assert loaded["product"]["b3d"].get("grows") is True
    growing = {n for n, f in loaded["family"].items() if f.get("grows")}
    assert growing == {"b3d-interface", "b3d-every-tick", "b3d-test"}


def test_the_validations_use_the_shared_build_and_source_checks_run_anywhere() -> None:
    families = manifest()["family"]
    assert families["b3d-source"]["platforms"] == "any"
    assert families["b3d-source"]["needs"] == ["python-env"]
    source_checks = {entry.check for entry in adapter.LISTS["b3d-source"]()}
    assert "b3d-source:data-names" in source_checks
    assert source_checks.isdisjoint({"b3d-source:b3d-data", "b3d-source:corpus"})
    assert families["b3d-validation"]["platforms"] == ["macos", "windows"]
    assert families["b3d-validation"]["needs"] == ["python-env", "b3d"]
    assert {entry.check for entry in adapter.LISTS["b3d-validation"]()} == {
        "b3d-validation:tuning",
        "b3d-validation:corpus",
    }


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
        "b3d-web-match",
        "b3d-web-drift",
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
    seen = {
        recording: set().union(*(pack.names_in(text, names) for text in texts))
        for recording, texts in exported.items()
    }
    found = adapter.recording_cards(seen, names, {})
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
    cards = adapter.recording_cards({"opening-play": set()}, {}, found)
    assert cards == {"opening-play": ["rust/cards/a/lea/counterspell.rs"]}


def test_interface_results_follow_each_tests_outcome() -> None:
    output: dict[str, object] = {
        "results": [
            {"test": "b3d:a/you/1440x900", "outcome": "pass"},
            {"test": "b3d:b/you/1440x900", "outcome": "fail"},
        ],
        "faults": [{"test": "b3d:b/you/1440x900", "rule": "reachable"}],
        "deferred": 0,
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


def test_frame_limits_need_a_complete_replay() -> None:
    wanted = [adapter.FRAME_LIMITS]
    family = "b3d-interface"
    output: dict[str, object] = {
        "results": [{"test": "b3d:a/you/1440x900", "outcome": "pass"}],
        "deferred": 1,
        "over_limits": [],
    }
    assert adapter.interface_results(output, family, wanted) == []
    output["deferred"] = 0
    assert adapter.interface_results(output, family, wanted) == [
        adapter.Result("b3d-interface:frame-limits", "pass")
    ]


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
    every["frame_allocations"] = 0
    return {"frame": index, "ms": index * 16, "phase": "rest", **every, **counts}


@pytest.fixture
def frame_profile(monkeypatch: pytest.MonkeyPatch) -> None:
    profile = calibration.Profile(
        "fixture", "a" * 40, "beryl3d-frame-native/v1", {}, {"base": "budgets.json"}
    )
    monkeypatch.setattr(frame_profiles, "native", lambda: profile)


@pytest.mark.usefixtures("frame_profile")
def test_gate_reports_budget_failures_and_missing_boards(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    output: str = json.dumps(
        {
            "boards": [
                {
                    "name": name,
                    "frames": [frame(0, resolved=1), frame(1, elements=3, resolved=1)],
                    "handle_allocations": 0,
                    "first_frame_allocation": None,
                }
                for name in ("held", "over", "baseline-only")
            ],
            "enhanced": [
                {
                    "name": f"{name} enhanced",
                    "frames": [
                        frame(0, particles=1, resolved=1),
                        frame(1, particles=1, resolved=1),
                    ],
                }
                for name in ("held", "over")
            ],
        }
    )
    board = gate.parse(output)[0]
    held: dict[str, gate.Budget] = {
        m.key: gate.Budget(max=gate.number(m.key, m.value) + 1, why="test budget")
        for m in gate.measure(board)
        if m.key != "frame_allocations"
    }
    budgets: gate.Budgets = {
        "boards": {
            "held": held,
            "over": {**held, "worst_frame_elements": {"max": 2, "why": "test budget"}},
            "baseline-only": held,
        },
        "enhanced": {"particles": {"max": 2, "why": "test budget"}},
        "scaling": {},
        "trend": {"adjacent_percent": 5, "weekly_percent": 10},
    }

    def run(command: list[str]) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess[str](command, 0, output, "")

    monkeypatch.setattr(adapter, "run_out", run)
    monkeypatch.setattr(gate, "read_budgets", lambda: budgets)
    results = adapter.gate_run(
        [f"b3d-gate:{name}" for name in ("held", "over", "absent", "baseline-only")]
    )
    assert [(r.check, r.outcome) for r in results] == [
        ("b3d-gate:held", "pass"),
        ("b3d-gate:over", "fail"),
        ("b3d-gate:absent", "incomplete"),
        ("b3d-gate:baseline-only", "incomplete"),
    ]
    assert "over its budget 2" in results[1].message
    assert "status 0 without a complete" in results[2].message
    assert "b3d-gate:over: over" in capsys.readouterr().err


@pytest.mark.parametrize(
    ("returncode", "stdout", "stderr", "evidence"),
    [
        (1, "", "", "without a complete"),
        (2, "runner progress", "runner error", "without a complete"),
        (0, "truncated report", "", "invalid board output"),
    ],
)
@pytest.mark.usefixtures("frame_profile")
def test_gate_runner_failures_report_incomplete_rows(
    returncode: int,
    stdout: str,
    stderr: str,
    evidence: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def run(command: list[str]) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(command, returncode, stdout, stderr)

    def budgets() -> gate.Budgets:
        return {
            "boards": {},
            "enhanced": {},
            "scaling": {},
            "trend": {"adjacent_percent": 5, "weekly_percent": 10},
        }

    monkeypatch.setattr(adapter, "run_out", run)
    monkeypatch.setattr(gate, "read_budgets", budgets)
    target = tmp_path / "results.jsonl"
    monkeypatch.setenv(RESULTS_ENV, str(target))
    checks = ["b3d-gate:pay-default", "b3d-gate:stress-verbs"]
    assert adapter.main(["b3d-gate", "run", *checks]) == 0
    rows = [json.loads(line) for line in target.read_text().splitlines()]
    assert [(row["check"], row["outcome"]) for row in rows] == [
        (check, "incomplete") for check in checks
    ]
    assert all(f"status {returncode}" in row["message"] for row in rows)
    assert all(evidence in row["message"] for row in rows)
    log = capsys.readouterr().err
    assert f"gate exited with status {returncode}" in log
    assert stdout in log and stderr in log


def test_native_gate_without_a_profile_cannot_pass(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("IGNITE_MACHINE", "unknown-test-verifier")
    monkeypatch.setattr(adapter, "cards_by_recording", lambda: {})
    monkeypatch.setattr(adapter, "gate_boards", lambda: ["stress-255"])
    (listed,) = adapter.gate_list()
    assert listed.check == "b3d-gate:stress-255"
    assert not listed.digest
    (result,) = adapter.gate_run([listed.check])
    assert result.outcome == "incomplete"
    assert "unknown-test-verifier" in result.message


def test_b3d_clippy_preserves_diagnostics_after_an_unsuccessful_exit(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    output: str = "\n".join(
        json.dumps(row)
        for row in [
            {
                "reason": "compiler-artifact",
                "package_id": "path+file:///w/crates/kit_scene#0.1.0",
            },
            {
                "reason": "compiler-message",
                "package_id": "path+file:///w/crates/app_client#0.1.0",
                "message": {
                    "level": "error",
                    "rendered": "error: mismatched types",
                    "spans": [
                        {
                            "file_name": "crates/app_client/src/lib.rs",
                            "line_start": 17,
                            "is_primary": True,
                        }
                    ],
                },
            },
        ]
    )

    def run(command: list[str], **options: object) -> subprocess.CompletedProcess[str]:
        assert options["cwd"] == adapter.WORKSPACE
        print("Finished `dev` profile", file=sys.stderr)
        return subprocess.CompletedProcess(command, 1, output)

    monkeypatch.setattr(subprocess, "run", run)
    results = adapter.clippy_run(["b3d-clippy:kit_scene", "b3d-clippy:app_client"])
    assert results == [
        Result(
            "b3d-clippy:kit_scene",
            "incomplete",
            "cargo clippy exited with status 1 before confirming the package result",
        ),
        Result(
            "b3d-clippy:app_client",
            "fail",
            "error: mismatched types",
            "rust/beryl3d/crates/app_client/src/lib.rs:17",
        ),
    ]
    log = capsys.readouterr().err
    assert "Finished `dev` profile" in log
    assert "b3d-clippy:kit_scene: cargo clippy exited with status 1" in log
    assert "b3d-clippy:app_client: error: mismatched types" in log


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


def test_timeline_results_name_each_fixture_over_its_allowance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(timeline, "WEB_COPIES", tmp_path)
    for name in ("allowed", "differs", "empty"):
        timeline.copy_path(name).write_text("")
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
        found, KEYS, allowed, ["allowed", "differs", "empty"]
    )
    assert [(r.check, r.outcome) for r in results] == [
        ("b3d-timeline:allowed", "pass"),
        ("b3d-timeline:differs", "fail"),
        ("b3d-timeline:empty", "pass"),
    ]
    assert "differs/p0" in results[1].message
    assert "committed copy" in results[1].message


def test_a_fixture_with_no_committed_web_timeline_fails_with_the_command_that_writes_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(timeline, "WEB_COPIES", tmp_path)
    results = adapter.timeline_results({}, KEYS, {}, ["allowed"])
    assert results[0].outcome == "fail"
    assert "--refresh-web" in results[0].message


def test_committed_web_timelines_round_trip_and_prune(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(timeline, "WEB_COPIES", tmp_path / "web")
    lines = [
        json.dumps({"recording": r, "seat": s, **step(start)})
        for r, s, start in [("a", "you", 0), ("a", "opponent", 5), ("b", "you", 9)]
    ]
    web = timeline.parse(lines)
    (tmp_path / "web").mkdir()
    timeline.copy_path("gone").write_text("")
    assert timeline.write_copies(web, {"a", "b"}, prune=True) == 2
    assert not timeline.copy_path("gone").exists()
    assert '"recording"' not in timeline.copy_path("a").read_text()
    assert timeline.load_copy("a") == {
        k: v for k, v in web.items() if k.startswith("a/")
    }
    assert timeline.load_copy("c") is None


def test_web_timeline_drift_counts_every_step_that_is_not_equal() -> None:
    committed = [
        {"recording": "a", "seat": "you", **step(s), "step": i}
        for i, s in enumerate((0, 100, 200))
    ]
    assert timeline.drifted(committed, committed) is None
    live = [committed[0], {**committed[1], "end": 101}]
    found = timeline.drifted(live, committed)
    assert found is not None
    assert found.count == 2
    assert found.web == live[1] and found.native == committed[1]


def test_timeline_drift_fails_a_fixture_whose_web_timeline_moved(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from beryl_tools.ignite import beryl3d_timeline_drift as drift

    monkeypatch.setattr(timeline, "WEB_COPIES", tmp_path)
    live = timeline.parse([json.dumps({"recording": "a", "seat": "you", **step(0)})])
    timeline.write_copies(live, {"a"}, prune=False)
    assert drift.drift_one("a", live).outcome == "pass"
    moved = timeline.parse([json.dumps({"recording": "a", "seat": "you", **step(5)})])
    failed = drift.drift_one("a", moved)
    assert failed.check == "b3d-timeline-drift:a"
    assert failed.outcome == "fail"
    assert "a/you: 1 step(s)" in failed.message
    assert "--refresh-web" in failed.message
    assert drift.drift_one("b", live).outcome == "fail"


KEYS: list[tuple[str, str]] = [("allowed", "p0"), ("differs", "p0")]


def test_format_results_blame_the_package_that_holds_the_file() -> None:
    root = paths.REPO / "rust" / "beryl3d" / "crates"
    output = f"Diff in {root}/app_client/src/lib.rs:12:\n-a\n+b\n"
    dirs = {
        "app_client": "rust/beryl3d/crates/app_client",
        "beryl_inspect": "rust/beryl3d/crates/beryl_inspect",
    }
    results = adapter.format_results(1, output, dirs, ["app_client", "beryl_inspect"])
    assert results == [
        Result(
            "b3d-format:app_client",
            "fail",
            "not formatted (run `just fmt`):\nrust/beryl3d/crates/app_client/src/lib.rs",
            "rust/beryl3d/crates/app_client/src/lib.rs:12",
        ),
        Result("b3d-format:beryl_inspect", "pass"),
    ]


# The web-match families: Beryl 3D's look held against the web interface's.


# What every Beryl 3D family reads under ui/: the fixtures' definitions and the generated
# protocol types, which the recordings are made from, and the audio manifests that Beryl 3D's
# build script and `audio/files.rs` read.
UI_READ_BY_EVERY_FAMILY = [
    "ui/src/generated/**",
    "ui/fixtures.json",
    "ui/src/audio/manifest.json",
    "ui/src/audio/loops.json",
]


def test_every_file_under_ui_that_beryl_3d_embeds_or_reads_is_an_input_of_every_family() -> (
    None
):
    """The `check-inputs` source rule listed these audio reads in its allowlist instead of
    failing on them, and Beryl 3D's families name their inputs by hand, so this test compares
    them with the files under ui/ that its sources embed with `include_str!` or its build
    scripts read."""
    pattern = re.compile(r"ui/[A-Za-z0-9_./-]+\.json")
    read: set[str] = set()
    for source in (paths.REPO / "rust" / "beryl3d" / "crates").rglob("*.rs"):
        if "target" in source.parts:
            continue
        for line in source.read_text().splitlines():
            if (
                source.name == "build.rs"
                or "include_str!" in line
                or "include_bytes!" in line
            ):
                read.update(pattern.findall(line))
    assert read, "no read of a file under ui/ found: the scan is broken"
    families = manifest()
    builders = [families["product"]["b3d"], *families["family"].values()]
    # Formatting and the source rules read Rust text, never what a build embeds.
    reads_text_only = [
        families["family"]["b3d-format"],
        families["family"]["b3d-source"],
    ]
    for declared in builders:
        if declared in reads_text_only:
            continue
        inputs = declared.get("inputs", [])
        assert isinstance(inputs, list)
        if "rust/beryl3d/**" not in inputs and not any(
            str(i).startswith("rust/beryl3d/crates/") for i in inputs
        ):
            continue
        missing = sorted(
            f for f in read if f not in inputs and not f.startswith("ui/src/generated/")
        )
        assert not missing, (declared.get("kind"), missing)


def test_no_regression_family_reads_the_web_interface() -> None:
    """A change to the web interface must run none of Beryl 3D's checks (docs/beryl-3d.md,
    decision C17), so a regression family declares nothing under ui/ but what every Beryl 3D
    family reads. The web side of a comparison is a committed copy under data/beryl3d/, and an
    exploration family compares that copy with the live web interface."""
    for name, family in manifest()["family"].items():
        if family["kind"] != "regression":
            continue
        declared = family.get("inputs", [])
        assert isinstance(declared, list)
        under_ui = [str(i) for i in declared if str(i).startswith("ui/")]
        assert set(under_ui) <= set(UI_READ_BY_EVERY_FAMILY), (name, under_ui)
        assert "ui-deps" not in str(family.get("needs", [])), name


def test_web_match_and_timeline_read_their_committed_web_copies() -> None:
    families = manifest()["family"]
    match = [str(i) for i in families["b3d-web-match"]["inputs"]]  # type: ignore[union-attr]
    assert "tools/src/beryl_tools/beryl3d/match/**" in match
    assert "data/beryl3d/match/web/**" in match
    parity = [str(i) for i in families["b3d-timeline"]["inputs"]]  # type: ignore[union-attr]
    assert "data/beryl3d/timeline/web/**" in parity


def test_timeline_drift_is_an_exploration_family_that_reads_the_web_timeline() -> None:
    family = manifest()["family"]["b3d-timeline-drift"]
    assert family["kind"] == "exploration"
    inputs = str(family["inputs"])
    assert "ui/src/**" in inputs and "ui/scripts/timeline.mjs" in inputs
    assert "tools/src/beryl_tools/ignite/beryl3d_timeline_drift.py" in inputs
    assert str(family["next"]).endswith("beryl3d b3d-timeline-drift next")


def test_web_drift_is_an_exploration_family_that_reads_the_web_interface() -> None:
    family = manifest()["family"]["b3d-web-drift"]
    assert family["kind"] == "exploration"
    assert "browser-pages" in str(family["resources"])
    assert "ui/src/**" in str(family["inputs"])
    assert str(family["next"]).endswith("beryl3d b3d-web-drift next")


def test_web_match_has_a_check_for_each_reference_target() -> None:
    listed = web_match.list_web_match({"screen/start": ["rust/cards/a/lea/island.rs"]})
    names = [entry.check for entry in listed]
    assert len(names) == len(set(names)) == len(reference.targets())
    assert "b3d-web-match:screen/start/you/1600x900@2" in names
    assert "b3d-web-match:screen/start/you/393x759" in names
    start = next(e for e in listed if e.check.endswith("screen/start/you/393x759"))
    assert start.inputs == [
        "data/beryl3d/match/web/screen_start_you_393x759.json",
        "rust/cards/a/lea/island.rs",
    ]
    # A mid-game target is named by its test and its moment, apart from the recording's end.
    assert "b3d-web-match:combat-attackers/you/1600x900@2#step:163.1.2+205ms" in names
    document = reference.load_ratchet()
    for target in reference.targets():
        name = reference.entry_name(target)
        assert reference.path(target).is_file(), name
        assert reference.entry(document, name) is not None, name
        assert reference.by_check_name(reference.check_name(target)) == target


def test_web_match_fails_only_when_a_count_rises(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    chosen: Target = reference.targets()[0]
    name = reference.check_name(chosen)
    web = reference.load(chosen)
    assert web is not None
    check = f"b3d-web-match:{name}"

    def entries(**counts: int) -> dict[str, object]:
        held = {n: 0 for n in reference.COUNTS} | counts
        return {"targets": {chosen.test_name(): held}}

    monkeypatch.setattr(web_match, "b3d_records", lambda _t: [dict(e) for e in web])
    assert web_match.match_one(name, entries()) == Result(check, "pass")

    monkeypatch.setattr(
        web_match, "b3d_records", lambda _t: [dict(e) for e in web[:-1]]
    )
    failed = web_match.match_one(name, entries())
    assert failed.outcome == "fail"
    assert "unmatched 1 > 0" in failed.message
    assert chosen.test_name() in failed.message
    assert web_match.match_one(name, entries(unmatched=1)).outcome == "pass"
    missing = web_match.match_one(name, {"targets": {}})
    assert missing.outcome == "fail" and "no entry" in missing.message
    assert web_match.match_one("nowhere/you/1x1", entries()).outcome == "fail"


def test_a_count_below_its_entry_fails_the_score_with_the_command_to_lower_it() -> None:
    document = {
        "targets": {
            "t": {
                "unmatched": 5,
                "boxes": 5,
                "styles": 5,
                "unsupported": 5,
                "pixels": 1000,
            }
        }
    }
    held = {"unmatched": 5, "boxes": 5, "styles": 5, "unsupported": 5, "pixels": 1000}
    # A pixel count within its slack of the entry is not a change; the other counts are exact.
    near = held | {"pixels": 995}
    assert score.judge({"t": near}, document) == ([], [], [])
    over, under, lines = score.judge({"t": held | {"boxes": 6}}, document)
    assert (over, under) == (["t"], []) and "boxes 6 > 5" in lines[0]
    over, under, lines = score.judge({"t": held | {"styles": 4}}, document)
    assert (over, under) == ([], ["t"]) and "styles 4 < 5" in lines[0]
    over, _, lines = score.judge({"new": held}, document)
    assert over == ["new"] and "not listed" in lines[0]


def test_update_lowers_entries_and_raises_or_adds_only_with_raise() -> None:
    entry = {"unmatched": 5, "boxes": 5, "styles": 5, "unsupported": 5, "pixels": 1000}
    document = {"targets": {"lower": entry, "rise": entry, "kept": entry}}
    measured = {
        "lower": entry | {"boxes": 2},
        "rise": entry | {"boxes": 9},
        "new": entry,
    }
    entries, refused = score.updated(measured, document, raise_=False)
    assert entries["lower"]["boxes"] == 2
    assert entries["rise"]["boxes"] == 5 and "new" not in entries
    assert sorted(refused) == ["new", "rise"]
    assert entries["kept"] == entry
    entries, refused = score.updated(measured, document, raise_=True)
    assert entries["rise"]["boxes"] == 9 and "new" in entries and not refused


def test_the_committed_reference_is_trimmed_sorted_and_one_element_to_a_line() -> None:
    target = reference.targets()[0]
    text = reference.path(target).read_text()
    parsed = json.loads(text)
    assert parsed["target"] == target.test_name()
    assert text.count("\n") == len(parsed["elements"]) + 2
    assert all("z" not in e for e in parsed["elements"])
    assert reference.document(target, parsed["elements"]) == text


def test_a_drift_target_is_played_again_only_after_its_inputs_change(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(web_match, "web_digest", lambda: "one")
    first = web_match.next_web_drift()
    assert [c["check"] for c in first][0].startswith("b3d-web-drift:")
    assert first == web_match.next_web_drift()
    monkeypatch.setattr(web_match, "web_digest", lambda: "two")
    second = web_match.next_web_drift()
    assert [c["check"] for c in second] == [c["check"] for c in first]
    assert all(a["digest"] != b["digest"] for a, b in zip(first, second, strict=True))
