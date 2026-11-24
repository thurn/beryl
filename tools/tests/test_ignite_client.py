"""The client's families for Ignite: what each check reads, how each tool's output becomes one
result per check, and that the adapter imports none of the orchestration the switch deletes.
Running the programs is left to the families' own runs, which need the `client` build.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from beryl_tools import calibration, paths
from beryl_tools.client import frame_profiles, gate, pack, trend
from beryl_tools.ignite import RESULTS_ENV, Result
from beryl_tools.ignite import client as adapter

MANIFEST: Path = paths.REPO / "ignite" / "checks" / "client.toml"

FORBIDDEN: list[str] = str(
    "tokens lanes step_runner selection rust_fit python_fit runlog runstore verdicts budget "
    "hang flaky bisect slow_ci sweep nightly warm caches incidents deadline clock check_value "
    "checks"
).split()


def manifest() -> dict[str, dict[str, dict[str, object]]]:
    return tomllib.loads(MANIFEST.read_text())


def test_the_adapter_imports_no_orchestration() -> None:
    code = (
        "import sys, beryl_tools.ignite.client; "
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
        if family["kind"] == "exploration":
            assert str(family["next"]).endswith(f"client {name} next")
        # The corpus streams results through its own command dispatch.
        if name != "client-corpus":
            assert name in adapter.LISTS and name in adapter.RUNS, name
        assert family["list"] == (
            f"uv run --quiet --project tools python -m beryl_tools.ignite.client {name} list"
        )
        assert str(family["run"]).endswith(f"client {name} run {{checks}}")


def test_the_programs_that_reread_their_grant_ask_for_one_that_grows() -> None:
    """The recorder, replay checks and Nextest's parallel replay tests follow growing grants."""
    loaded = manifest()
    assert loaded["product"]["client"].get("grows") is True
    growing = {n for n, f in loaded["family"].items() if f.get("grows")}
    assert growing == {
        "client-interface",
        adapter.FRAME_LIMITS_FAMILY,
        "client-every-tick",
        "client-test",
    }


def test_the_validations_use_the_shared_build_and_source_checks_run_anywhere() -> None:
    families = manifest()["family"]
    assert families["client-source"]["platforms"] == "any"
    assert families["client-source"]["needs"] == ["python-env"]
    source_checks = {entry.check for entry in adapter.LISTS["client-source"]()}
    assert "client-source:data-names" in source_checks
    assert source_checks.isdisjoint({"client-source:b3d-data", "client-source:corpus"})
    assert families["client-validation"]["platforms"] == ["macos", "windows"]
    assert families["client-validation"]["needs"] == ["python-env", "client"]
    assert {entry.check for entry in adapter.LISTS["client-validation"]()} == {
        "client-validation:tuning",
        "client-validation:corpus",
    }


def test_windows_replay_and_stress_checks_defer_until_after_promotion() -> None:
    families = manifest()["family"]
    for name in (
        "client-interface",
        "client-every-tick",
        "client-test",
        "client-corpus",
        "client-gate",
        adapter.FRAME_LIMITS_FAMILY,
        "client-frame-counts",
        "client-frame-trend",
    ):
        assert families[name]["defer_platforms"] == ["windows"], name
    for name in ("client-clippy", "client-validation"):
        assert families[name]["platforms"] == ["macos", "windows"], name
        assert "defer_platforms" not in families[name], name


def test_frame_budgets_are_advisory_and_client_expectations_are_test_paths() -> None:
    families = manifest()["family"]
    for name in (adapter.FRAME_LIMITS_FAMILY, "client-gate"):
        assert families[name]["kind"] == "exploration", name
    config = tomllib.loads((paths.REPO / "ignite.toml").read_text())
    assert {
        "data/client/budgets.json",
        "data/client/gates.json",
        "data/client/conventions/**",
        "data/client/screens.json",
    } <= set(config["repository"]["test_paths"])


def test_the_families_that_read_recordings_name_every_crate_the_recorder_is_built_from() -> (
    None
):
    """the client reaches the main workspace by path, which `crate:` does not follow, so these
    families name the main crates themselves. A change to `beryl_session` must reach them.
    """
    crates = adapter.main_crates()
    assert "engine/crates/beryl_session/**" in crates
    for name in (
        "client-interface",
        adapter.FRAME_LIMITS_FAMILY,
        "client-every-tick",
        "client-gate",
        "client-frame-counts",
    ):
        inputs = manifest()["family"][name]["inputs"]
        assert isinstance(inputs, list)
        named = [i for i in inputs if str(i).startswith("engine/crates/")]
        assert named == crates, name
        # A card file is each check's own input, never the family's.
        assert not [
            i for i in inputs if str(i).startswith("engine/cards/") and "*/" not in i
        ]


def test_a_recording_reads_the_cards_either_seat_saw_and_no_other() -> None:
    names = {
        "Counterspell": "engine/cards/a/lea/counterspell.rs",
        "Island": "engine/cards/a/lea/island.rs",
        "Fable of the Mirror-Breaker": "engine/cards/b/neo/fable_of_the_mirror_breaker.rs",
        "Double Major": "engine/cards/c/stx/double_major.rs",
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
            "engine/cards/a/lea/counterspell.rs",
            "engine/cards/a/lea/island.rs",
        ],
        "verbs-zones": [],
    }
    assert not any("double_major" in c for cards in found.values() for c in cards)


def test_an_agents_fixture_reads_its_whole_deck_and_a_fuzz_deck_every_card() -> None:
    files = ["engine/cards/a/lea/counterspell.rs", "engine/cards/a/lea/island.rs"]
    defined: list[dict[str, object]] = [
        {"name": "opening-play", "decks": ["wu-control", "wu-control"]},
        {"name": "fuzzed", "decks": ["fuzz:3", "wu-control"]},
        {"name": "table-opening", "table": {"deck": "small.txt"}},
        {"name": "game-over", "board": "game-over"},
    ]
    decks = {"wu-control": "Deck\n4 Counterspell\n", "small.txt": "2 Island\n"}
    found = adapter.fixture_decks(defined, decks, files)
    assert found == {
        "opening-play": {"engine/cards/a/lea/counterspell.rs"},
        "fuzzed": set(files),
        "table-opening": {"engine/cards/a/lea/island.rs"},
    }
    cards = adapter.recording_cards({"opening-play": set()}, {}, found)
    assert cards == {"opening-play": ["engine/cards/a/lea/counterspell.rs"]}


def test_interface_results_follow_each_tests_outcome() -> None:
    output: dict[str, object] = {
        "results": [
            {"test": "client:a/you/1440x900", "outcome": "pass"},
            {"test": "client:b/you/1440x900", "outcome": "fail"},
        ],
        "faults": [{"test": "client:b/you/1440x900", "rule": "reachable"}],
        "deferred": 0,
        "over_limits": ["a/you/1440x900 drew 900 frames, over `most_frames` 800"],
    }
    wanted = ["a/you/1440x900", "b/you/1440x900", "c/you/1440x900"]
    results = adapter.interface_results(output, "client-interface", wanted)
    outcomes = {r.check: r.outcome for r in results}
    assert outcomes == {
        "client-interface:a/you/1440x900": "pass",
        "client-interface:b/you/1440x900": "fail",
    }
    assert "reachable" in results[1].message


def test_frame_limits_have_a_separate_native_family_with_all_card_inputs(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    families = manifest()["family"]
    aggregate = families[adapter.FRAME_LIMITS_FAMILY]
    interface = families["client-interface"]
    assert aggregate["platforms"] == interface["platforms"] == ["macos", "windows"]
    assert aggregate["needs"] == interface["needs"]
    assert aggregate["inputs"] == interface["inputs"]
    config = tomllib.loads((paths.REPO / "ignite.toml").read_text())
    assert adapter.FRAME_LIMITS_FAMILY in config["machine"]["cygnus"]["families"]
    monkeypatch.setattr(
        adapter, "test_names", lambda: ["a/you/1440x900", "b/you/1440x900"]
    )
    monkeypatch.setattr(
        adapter,
        "cards_by_recording",
        lambda: {"a": ["card-a"], "b": ["card-a", "card-b"], "c": ["card-c"]},
    )
    assert [entry.check for entry in adapter.LISTS["client-interface"]()] == [
        "client-interface:a/you/1440x900",
        "client-interface:b/you/1440x900",
    ]
    assert adapter.LISTS[adapter.FRAME_LIMITS_FAMILY]() == [
        adapter.Listed(adapter.FRAME_LIMITS, ["card-a", "card-b"])
    ]


@pytest.mark.parametrize(
    ("family", "suite", "runner", "checks", "outcome"),
    [
        (
            adapter.FRAME_LIMITS_FAMILY,
            "corpus",
            "frame_limits_run",
            [adapter.FRAME_LIMITS],
            "pass",
        ),
        (
            "client-gate",
            "boards",
            "gate_run",
            ["client-gate:pay-default", "client-gate:stress-255"],
            "fail",
        ),
    ],
)
def test_budget_exploration_runs_only_its_verified_checkout(
    family: str,
    suite: str,
    runner: str,
    checks: list[str],
    outcome: str,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    tmp_path: Path,
) -> None:
    output = tmp_path / "results.jsonl"
    monkeypatch.setenv(RESULTS_ENV, str(output))
    monkeypatch.setattr(trend, "current_commit", lambda: "a" * 40)
    assert adapter.main([family, "next"]) == 0
    (listing,) = capsys.readouterr().out.splitlines()
    check = json.loads(listing)["check"]
    assert check == f"{family}:{suite}/" + "a" * 40

    def run(wanted: list[str]) -> list[Result]:
        assert wanted == checks
        return [
            Result(name, outcome if index == len(wanted) - 1 else "pass", "over budget")
            for index, name in enumerate(wanted)
        ]

    monkeypatch.setattr(adapter, runner, run)
    monkeypatch.setattr(adapter, "gate_boards", lambda: ["pay-default", "stress-255"])
    assert adapter.main([family, "run", check]) == int(outcome == "fail")

    monkeypatch.setattr(trend, "current_commit", lambda: "b" * 40)
    assert adapter.main([family, "run", check]) == 0
    measured, skipped = [json.loads(line) for line in output.read_text().splitlines()]
    assert measured["check"] == skipped["check"] == check
    assert measured["outcome"] == outcome
    if outcome == "fail":
        assert measured["message"] == "client-gate:stress-255: over budget"
    assert skipped["outcome"] == "incomplete"
    assert "b" * 40 in skipped["message"]


@pytest.mark.parametrize(
    "native_report,expected",
    [
        ({"tests": 2, "over_limits": []}, "pass"),
        ({"tests": 2, "over_limits": ["median exceeded"]}, "fail"),
        ({"tests": 0, "over_limits": []}, ""),
        ({}, ""),
    ],
)
def test_frame_limits_publish_only_a_complete_native_verdict(
    native_report: dict[str, object],
    expected: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    output = tmp_path / "results.jsonl"
    monkeypatch.setenv(RESULTS_ENV, str(output))

    def fake(command: list[str]) -> subprocess.CompletedProcess[str]:
        assert command[1:] == ["test", "--frame-limits", "--json"]
        return subprocess.CompletedProcess(
            command, int(expected != "pass"), json.dumps(native_report), ""
        )

    monkeypatch.setattr(adapter, "run_out", fake)
    status = adapter.main([adapter.FRAME_LIMITS_FAMILY, "run", adapter.FRAME_LIMITS])
    assert status == int(expected == "fail")
    verdicts = [json.loads(line) for line in output.read_text().splitlines()]
    if expected:
        assert len(verdicts) == 1
        assert verdicts[0]["check"] == adapter.FRAME_LIMITS
        assert verdicts[0]["outcome"] == expected
        if expected == "fail":
            assert verdicts[0]["message"] == "median exceeded"
    else:
        assert verdicts == []


def test_a_run_of_some_tests_plays_only_their_common_prefix() -> None:
    assert adapter.common_prefix(["a/b/you/1", "a/b/opponent/1"]) == "a/b/"
    assert adapter.common_prefix(["a/you/1", "b/you/1"]) == ""


def test_a_batch_across_recordings_runs_only_its_recordings(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
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
        rows = [{"test": f"client:{t}", "outcome": "pass"} for t in names]
        return subprocess.CompletedProcess(
            command, 0, json.dumps({"results": rows}), ""
        )

    monkeypatch.setattr(adapter, "run_out", fake)
    output = tmp_path / "results.jsonl"
    monkeypatch.setenv(RESULTS_ENV, str(output))
    checks = ["client-interface:a/you/1440x900", "client-interface:c/opponent/1440x900"]
    assert adapter.main(["client-interface", "run", *checks]) == 0
    assert sorted(played) == ["a/you/1440x900", "c/opponent/1440x900"]
    assert [json.loads(line) for line in output.read_text().splitlines()] == [
        {"check": check, "outcome": "pass"} for check in checks
    ]


def frame(index: int, **counts: float) -> dict[str, object]:
    keys = [
        *gate.COUNTERS,
        *gate.LOAD,
        *gate.BLENDED_AREA,
        *gate.BLENDED_AREAS_BY_BAND,
        "frame_allocations",
    ]
    every = {c: 1.0 for c in keys}
    every["frame_allocations"] = 0
    return {"frame": index, "ms": index * 16, "phase": "rest", **every, **counts}


@pytest.fixture
def frame_profile(monkeypatch: pytest.MonkeyPatch) -> None:
    profile = calibration.Profile(
        "fixture", "a" * 40, "client-frame-native/v1", {}, {"base": "budgets.json"}
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
        [f"client-gate:{name}" for name in ("held", "over", "absent", "baseline-only")]
    )
    assert [(r.check, r.outcome) for r in results] == [
        ("client-gate:held", "pass"),
        ("client-gate:over", "fail"),
        ("client-gate:absent", "incomplete"),
        ("client-gate:baseline-only", "incomplete"),
    ]
    assert "over its budget 2" in results[1].message
    assert "status 0 without a complete" in results[2].message
    assert "client-gate:over: over" in capsys.readouterr().err


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
    checks = ["client-gate:pay-default", "client-gate:stress-verbs"]
    assert adapter.main(["client-gate", "run", *checks]) == 0
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
    assert listed.check == "client-gate:stress-255"
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
    results = adapter.clippy_run(
        ["client-clippy:kit_scene", "client-clippy:app_client"]
    )
    assert results == [
        Result(
            "client-clippy:kit_scene",
            "incomplete",
            "cargo clippy exited with status 1 before confirming the package result",
        ),
        Result(
            "client-clippy:app_client",
            "fail",
            "error: mismatched types",
            "client/crates/app_client/src/lib.rs:17",
        ),
    ]
    log = capsys.readouterr().err
    assert "Finished `dev` profile" in log
    assert "client-clippy:kit_scene: cargo clippy exited with status 1" in log
    assert "client-clippy:app_client: error: mismatched types" in log


def test_format_results_blame_the_package_that_holds_the_file() -> None:
    root = paths.REPO / "client" / "crates"
    output = f"Diff in {root}/app_client/src/lib.rs:12:\n-a\n+b\n"
    dirs = {
        "app_client": "client/crates/app_client",
        "beryl_inspect": "client/crates/beryl_inspect",
    }
    results = adapter.format_results(1, output, dirs, ["app_client", "beryl_inspect"])
    assert results == [
        Result(
            "client-format:app_client",
            "fail",
            "not formatted (run `just fmt`):\nclient/crates/app_client/src/lib.rs",
            "client/crates/app_client/src/lib.rs:12",
        ),
        Result("client-format:beryl_inspect", "pass"),
    ]


def test_native_audio_manifests_are_inputs_to_the_native_build_checks() -> None:
    """the client names its shared inputs by hand, including JSON read at compile time."""
    pattern = re.compile(r"data/audio/[A-Za-z0-9_./-]+\.json")
    read: set[str] = set()
    for source in (paths.REPO / "client" / "crates").rglob("*.rs"):
        if "target" in source.parts:
            continue
        for line in source.read_text().splitlines():
            if (
                source.name == "build.rs"
                or "include_str!" in line
                or "include_bytes!" in line
            ):
                read.update(pattern.findall(line))
    assert read == {"data/audio/manifest.json", "data/audio/loops.json"}
    declared = manifest()
    families = declared["family"]
    assert {
        "b3d-timeline",
        "b3d-timeline-drift",
        "b3d-web-match",
        "b3d-web-drift",
    }.isdisjoint(families)
    for name, entry in (declared["product"] | families).items():
        inputs = entry.get("inputs", [])
        assert isinstance(inputs, list)
        assert not any(str(path).startswith("ui/") for path in inputs), name
        if name in {"client-format", "client-source"}:
            continue
        if any(str(path).startswith("client/") for path in inputs):
            assert read.issubset(inputs), name
