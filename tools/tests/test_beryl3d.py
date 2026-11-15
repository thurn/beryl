"""Beryl 3D's paths select its own lane and no other, and no other path selects it
(docs/beryl-3d.md, 'Checks and isolation')."""

from __future__ import annotations

import pytest

from beryl_tools import checks, oracles, paths, rust_tests, selection, slow_ci
from beryl_tools.beryl3d import cli as beryl3d_cli
from beryl_tools.beryl3d import paths as beryl3d_paths
from beryl_tools.slow_ci import LANES

BERYL3D: list[str] = [
    "rust/beryl3d/crates/beryl_scene/src/lib.rs",
    "data/beryl3d/gates.json",
    "rust/beryl3d/crates/beryl_3d/ios/App.swift",
    "rust/beryl3d/Cargo.lock",
    "rust/beryl3d/AGENTS.md",
]
MAIN: list[list[str]] = [
    ["rust/crates/beryl_session/src/lib.rs"],
    ["ui/src/stage/beat.ts"],
    ["data/ui-conventions.json"],
    ["rust/Cargo.lock"],
    ["justfile"],
    ["docs/beryl-3d.md"],
]
SCRIPT_INPUTS = r"^rust/(Cargo\.(toml|lock)|crates/beryl_cli/|cards/)"


def package(name: str, *deps: str) -> dict[str, object]:
    return {
        "name": name,
        "manifest_path": str(paths.RUST / "crates" / name / "Cargo.toml"),
        "dependencies": [{"name": d, "path": f"../{d}"} for d in deps],
    }


# A small main workspace, so the rust lane's packages come from no `cargo metadata`.
PACKAGES: list[dict[str, object]] = [
    package("beryl_kernel"),
    package("beryl_session", "beryl_kernel"),
    package("beryl_app", "beryl_session"),
    package("beryl_cli", "beryl_session"),
    package("beryl_bench", "beryl_kernel"),
]


def affected(changed: list[str] | None) -> list[str] | None:
    return rust_tests.affected_packages(changed, PACKAGES)


def fast(monkeypatch: pytest.MonkeyPatch, changed: list[str]) -> dict[str, checks.Lane]:
    """The fast check's lanes for `changed` on a Mac with no oracles built, by name."""
    monkeypatch.setattr(oracles, "built", lambda oracle: False)
    monkeypatch.setattr(checks.system, "WINDOWS", False)
    monkeypatch.setattr(checks, "affected_packages", affected)
    return {lane.name: lane for lane in checks.fast_lanes(changed)}


def slow(changed: list[str]) -> slow_ci.Plan:
    """Slow CI's plan when every lane's base is the same commit and `changed` changed since."""
    return slow_ci.choose(
        LANES,
        dict.fromkeys(LANES, "base"),
        {"base": changed},
        lambda lane: "",
        SCRIPT_INPUTS,
        affected,
    )


def test_beryl3d_paths_run_only_its_lane_and_the_source_rules(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assert set(fast(monkeypatch, BERYL3D)) == {"repo", "names", "beryl3d"}
    for path in BERYL3D:
        assert set(fast(monkeypatch, [path])) <= {"repo", "names", "beryl3d"}, path
        assert "beryl3d" in fast(monkeypatch, [path]), path
    assert set(slow(BERYL3D).why) == {"repo", "names"}
    assert not slow(BERYL3D).whole


def test_no_other_path_runs_the_beryl3d_lane(monkeypatch: pytest.MonkeyPatch) -> None:
    for changed in MAIN:
        assert "beryl3d" not in fast(monkeypatch, changed), changed
        assert "beryl3d" not in slow(changed).why, changed
    # Beryl 3D's tools are tools: a change to them runs the python lane, not Beryl 3D's.
    tools = ["tools/src/beryl_tools/beryl3d/lane.py"]
    assert {"python"} <= set(fast(monkeypatch, tools)) and "beryl3d" not in fast(
        monkeypatch, tools
    )


def rust_packages(lanes: dict[str, checks.Lane]) -> list[list[str]]:
    return [s.cmd for s in lanes["rust"].steps] if "rust" in lanes else []


def test_a_beryl3d_path_changes_no_main_lane(monkeypatch: pytest.MonkeyPatch) -> None:
    for changed in MAIN:
        alone = fast(monkeypatch, changed)
        both = fast(monkeypatch, [*changed, BERYL3D[0]])
        # Beryl 3D's paths add their own lane and the source rules' (`names`), and no other.
        assert set(alone) <= set(both), changed
        assert set(both) - set(alone) <= {"beryl3d", "names"}, changed
        assert rust_packages(both) == rust_packages(alone), changed
        assert set(slow([*changed, *BERYL3D]).why) == set(slow(changed).why), changed
    # Without the split, a path under rust/ in no package of the main workspace would make
    # the rust lane test every package.
    assert rust_packages(fast(monkeypatch, [*MAIN[0], BERYL3D[0]])) == rust_packages(
        fast(monkeypatch, MAIN[0])
    )
    assert affected([*MAIN[0], BERYL3D[0]]) is None


def test_the_browser_checks_ignore_beryl3d_paths() -> None:
    universe = ["ui-test:drag"]
    assert selection.choose("ui-test", BERYL3D, {}, universe, None, "base").idle()
    assert not selection.choose("ui-test", MAIN[0], {}, universe, None, "base").idle()


def test_split() -> None:
    own, rest = beryl3d_paths.split([*BERYL3D, *MAIN[0], "rust/beryl3dx/a.rs"])
    assert own == BERYL3D
    assert rest == [*MAIN[0], "rust/beryl3dx/a.rs"]


def test_b3d_lists_its_commands_and_names_the_nearest_unknown_one(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert beryl3d_cli.main([]) == 0
    listed = capsys.readouterr().out.splitlines()
    assert listed[0] == "just b3d"
    assert any(line.split()[:1] == ["lane"] for line in listed)
    assert beryl3d_cli.main(["lnae"]) == beryl3d_cli.USAGE
    assert "'lane'" in capsys.readouterr().err
    with pytest.raises(SystemExit):
        beryl3d_cli.main(["lane", "--help"])
    shown = capsys.readouterr().out
    assert "examples:" in shown and shown.count("just b3d lane") >= 2


def test_each_command_has_two_examples_or_a_program_that_prints_its_own() -> None:
    for command in beryl3d_cli.COMMANDS:
        if command.passthrough:
            assert not command.examples, command.name
        else:
            assert len(command.examples) == 2, command.name
