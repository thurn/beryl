"""The efficiency dashboard's paths select its own lane and `repo`, and no other path selects
its lane (docs/efficiency-dashboard.md, 'Isolation')."""

from __future__ import annotations

import pytest

from beryl_tools import checks, efficiency, oracles, paths, rust_tests, slow_ci
from beryl_tools.slow_ci import LANES

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
]
DASHBOARD: list[str] = [
    "efficiency/src/beryl_efficiency/cli.py",
    "efficiency/tests/test_cli.py",
    "efficiency/pyproject.toml",
    "data/efficiency-prices.json",
]
ELSEWHERE: list[list[str]] = [
    ["rust/crates/beryl_session/src/lib.rs"],
    ["tools/src/beryl_tools/checks.py"],
    ["tools/src/beryl_tools/efficiency.py"],
    ["ui/src/stage/beat.ts"],
    ["data/ui-conventions.json"],
    ["data/efficiency.txt"],
]
# The lanes a change to the dashboard must never run.
MAIN = {"python", "scale", "rust", "script", "ui", "names", "ui-test", "shots"}


def affected(changed: list[str] | None) -> list[str] | None:
    return rust_tests.affected_packages(changed, PACKAGES)


def fast(monkeypatch: pytest.MonkeyPatch, changed: list[str]) -> set[str]:
    """The fast check's lanes for `changed` on a Mac with no oracles built."""
    monkeypatch.setattr(oracles, "built", lambda oracle: False)
    monkeypatch.setattr(checks.system, "WINDOWS", False)
    monkeypatch.setattr(checks, "affected_packages", affected)
    return {lane.name for lane in checks.fast_lanes(changed)}


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


def test_dashboard_paths_run_only_its_lane_and_repo(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assert fast(monkeypatch, DASHBOARD) == {"repo", "efficiency"}
    for path in DASHBOARD:
        assert fast(monkeypatch, [path]) == {"repo", "efficiency"}, path
    plan = slow(DASHBOARD)
    assert set(plan.why) == {"repo", "names", "efficiency"}
    assert not plan.whole and not set(plan.why) & MAIN - {"names"}


def test_no_other_path_runs_the_efficiency_lane(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for changed in ELSEWHERE:
        assert "efficiency" not in fast(monkeypatch, changed), changed
        assert "efficiency" not in slow(changed).why, changed


def test_a_dashboard_path_changes_no_other_lane(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for changed in ELSEWHERE:
        alone = fast(monkeypatch, changed)
        assert fast(monkeypatch, [*changed, *DASHBOARD]) - {"efficiency"} == alone
        both = slow([*changed, *DASHBOARD]).why
        assert set(both) - {"efficiency"} == set(slow(changed).why), changed


def test_every_lane_includes_it_and_slow_ci_can_make_it() -> None:
    assert "efficiency" in LANES
    lane = slow_ci.lanes_for(None)["efficiency"]()
    assert [s.name for s in lane.steps] == ["black", "pyre", "pytest"]
    assert all(s.cwd == efficiency.PROJECT for s in lane.steps)
    assert "efficiency" in {lane.name for lane in checks.fast_lanes(None)}


def test_split() -> None:
    own, rest = efficiency.split([*DASHBOARD, "data/efficiency.txt", "efficiencyx/a"])
    assert own == DASHBOARD
    assert rest == ["data/efficiency.txt", "efficiencyx/a"]
