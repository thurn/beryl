import json
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from beryl_tools import checks, runlog, script_checks, selection, slow_ci, verdicts
from beryl_tools.selection import Coverage
from beryl_tools.slow_ci import LANES, Passes

NOW = datetime(2026, 11, 13, 12, tzinfo=timezone.utc)
CARD = "rust/cards/beryl_cards_dsk/dsk/hand_that_feeds.rs"
SCRIPT_INPUTS = r"^rust/(Cargo\.(toml|lock)|crates/beryl_cli/|cards/)"
EVERYTHING: list[str] = [p for p in LANES if p != "scale"]


def plan(
    changed: list[str] | None,
    stale: str = "",
    packages: list[str] | None = None,
) -> slow_ci.Plan:
    """The plan when every lane's base is the same commit and `changed` changed since."""
    return slow_ci.choose(
        LANES,
        dict.fromkeys(LANES, "base"),
        {"base": changed},
        lambda lane: stale,
        SCRIPT_INPUTS,
        lambda found: packages,
    )


def test_a_docs_change_runs_only_the_lanes_that_need_no_build() -> None:
    chosen = plan(["docs/checks.md", "AGENTS.md", ".beads/issues.jsonl"])
    assert set(chosen.why) == {"repo", "names"}
    assert set(chosen.left) == set(EVERYTHING) - {"repo", "names"}
    assert not chosen.whole


def test_a_card_file_runs_the_lanes_that_build_and_play_it() -> None:
    chosen = plan([CARD], packages=["beryl_app", "beryl_cards_dsk", "beryl_cli"])
    assert set(chosen.why) == {
        "repo",
        "names",
        "scale",
        "rust",
        "script",
        "app",
        "ui-smoke",
        "oracle",
    }
    # The rollout benchmark does not depend on the changed package.
    assert set(chosen.left) == {"python", "ui", "counts"}
    assert chosen.why["rust"] == f"{CARD} changed"


def test_the_python_lane_holds_the_scale_lanes_tests() -> None:
    chosen = plan([CARD, "tools/tests/test_card.py"], packages=None)
    assert "python" in chosen.why
    assert "scale" not in chosen.why and "scale" not in chosen.left


def test_a_data_file_the_ui_imports_runs_the_ui_and_browser_lanes() -> None:
    chosen = plan(["data/arena-motion.json"])
    assert {"python", "ui", "app", "ui-smoke"} <= set(chosen.why)
    assert "rust" not in chosen.why


def test_a_tools_change_reaches_other_lanes_only_through_their_fingerprints() -> None:
    changed = ["tools/src/beryl_tools/card_names.py"]
    assert set(plan(changed).why) == {"repo", "names", "python"}
    stale = plan(changed, stale="its tools changed")
    assert set(stale.why) == set(EVERYTHING)
    assert stale.whole == set(stale.why)


def test_a_path_no_rule_places_runs_every_lane_in_full() -> None:
    chosen = plan(["Makefile"])
    assert chosen.whole == set(EVERYTHING)
    assert chosen.why["rust"] == "Makefile changed, which no rule places"
    assert plan(None).whole == set(EVERYTHING)


def test_each_lane_counts_its_changes_from_its_own_base() -> None:
    bases: dict[str, str] = {lane: "new" for lane in LANES}
    bases["ui-smoke"] = "old"  # It failed at `new`, so it keeps its older pass.
    chosen = slow_ci.choose(
        LANES,
        bases,
        {"new": ["docs/checks.md"], "old": ["ui/src/game/combat.ts"]},
        lambda lane: "",
        SCRIPT_INPUTS,
        lambda found: None,
    )
    assert set(chosen.why) == {"repo", "names", "ui-smoke"}
    assert chosen.bases["ui-smoke"] == "old"


def lane(step: str) -> checks.Lane:
    return checks.Lane("names").step(
        step, [sys.executable, "-m", "beryl_tools.checks", step]
    )


SOURCES = {
    "card_names": "from . import citations\n",
    "citations": "x = 1\n",
    "reruns": "",
    "verdicts": "from . import slow_only\n",
    "slow_only": "",
    "shots": "y = 2\n",
}


def test_a_lanes_fingerprint_follows_the_modules_its_steps_import() -> None:
    names = lane("card-names")
    before = slow_ci.fingerprint(names, SOURCES, {}, "")
    unrelated = {**SOURCES, "shots": "y = 3\n"}
    assert slow_ci.fingerprint(names, unrelated, {}, "") == before
    imported = {**SOURCES, "citations": "x = 2\n"}
    assert slow_ci.fingerprint(names, imported, {}, "") != before
    judged = {**SOURCES, "slow_only": "z = 1\n"}
    assert slow_ci.fingerprint(names, judged, {}, "") != before
    assert slow_ci.fingerprint(names, SOURCES, {}, "uv.lock 2") != before
    assert slow_ci.fingerprint(lane("citations"), SOURCES, {}, "") != before


def test_a_by_command_runs_the_module_that_defines_it() -> None:
    browser = checks.ui_test_lane()
    assert slow_ci.entry_modules(browser, {"ui-test": "shots"}) == {"shots"}


def test_a_pass_records_its_lanes_and_keeps_the_others(tmp_path: Path) -> None:
    path = tmp_path / "passes.json"
    Passes(path).record(["rust", "repo"], "aaa", {"rust": "r1", "repo": "p1"})
    # A later run where rust failed records only repo.
    Passes(path).record(["repo"], "bbb", {"repo": "p1"})
    passes = Passes.load(path)
    assert passes.lanes["rust"] == {"commit": "aaa", "fingerprint": "r1"}
    assert passes.lanes["repo"]["commit"] == "bbb"
    assert passes.stale("repo", "p1") == ""
    assert passes.stale("rust", "r2") != ""
    assert passes.stale("ui", "u1") == "no pass on record"


def test_a_full_sweep_is_due_an_hour_after_the_last_began(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv(slow_ci.FULL_ENV, raising=False)
    passes = Passes(tmp_path / "passes.json")
    assert passes.full_due(NOW) == "no full sweep on record"
    passes.full = (NOW - timedelta(minutes=20)).isoformat()
    assert passes.full_due(NOW) == ""
    passes.full = (NOW - timedelta(minutes=61)).isoformat()
    assert "61 min ago" in passes.full_due(NOW)
    monkeypatch.setenv(slow_ci.FULL_ENV, "1")
    passes.full = NOW.isoformat()
    assert passes.full_due(NOW) != ""


def test_slow_ci_tells_which_lanes_passed(monkeypatch: pytest.MonkeyPatch) -> None:
    ok = checks.Lane("ok").step("true", [sys.executable, "-c", "pass"])
    bad = checks.Lane("bad").step(
        "false", [sys.executable, "-c", "raise SystemExit(1)"]
    )
    monkeypatch.setattr("beryl_tools.bisect.after", lambda failed, started: None)
    passed: list[str] = []
    started = time.monotonic()
    assert checks.slow_run([ok, bad], 300, started, "affected", passed.extend) == 1
    assert passed == ["ok"]


@pytest.fixture
def chooser_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A checkout whose shots.json changed `attack`'s route since the base, with fresh
    coverage."""
    (tmp_path / "ui").mkdir()
    shots = {"shots": [{"name": "attack", "route": "/new"}, {"name": "block"}]}
    (tmp_path / "ui" / "shots.json").write_text(json.dumps(shots))
    monkeypatch.setattr(selection.paths, "REPO", tmp_path)
    old = {"shots": [{"name": "attack", "route": "/old"}, {"name": "block"}]}
    monkeypatch.setattr(checks, "git", lambda *args: json.dumps(old))
    monkeypatch.setattr(checks, "changed_paths", lambda base: ["ui/shots.json"])
    monkeypatch.setattr(selection, "diff", lambda *args, under="": {})
    monkeypatch.setattr(runlog, "read_log", lambda since: [])
    monkeypatch.setattr(verdicts, "cached_history", lambda: {})
    fresh = Coverage("c0", datetime.now(timezone.utc), {}, {})
    monkeypatch.setattr(Coverage, "load", staticmethod(lambda: fresh))
    # Windows records no coverage, and these tests run there too.
    monkeypatch.setattr(
        selection, "stale", lambda found, now: "" if found else "no coverage"
    )


def test_slow_ci_plays_every_chosen_check_with_no_time_box(
    chooser_env: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    moved = verdicts.SlowOnly("shots:attack", "2026-11-11", 900.0, 30)
    monkeypatch.setattr(verdicts, "SLOW_ONLY", (moved,))
    chooser = selection.Chooser("shots", base="base")
    # Its seconds would overrun the fast check's box, and the fast check leaves it out.
    assert chooser.run([], []) == ["attack"]
    assert chooser.cut == []


def test_slow_ci_plays_every_check_when_coverage_cannot_place_the_change(
    chooser_env: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(Coverage, "load", staticmethod(lambda: None))
    chooser = selection.Chooser("shots", base="base")
    assert sorted(chooser.run([], [])) == ["attack", "block"]
    assert all(r.startswith("every:") for r in chooser.choice.reasons.values())


def test_slow_cis_lanes_leave_their_fast_only_checks_to_the_fast_check() -> None:
    lanes: dict[str, checks.Lane] = {
        name: make() for name, make in slow_ci.lanes_for(None).items()
    }

    fast_only = set(verdicts.FAST_ONLY)
    # The fast check runs the Python tests and the script checks; slow CI leaves them to it,
    # and keeps the python lane's formatter and type check.
    assert "pytest" in {s.name for s in checks.python_lane().steps}
    assert "black" in {s.name for s in lanes["python"].steps}
    assert lanes["scale"].steps == [] and lanes["script"].steps == []
    assert [s.name for s in script_checks.lane().steps] == ["script-check"]
    # A whole step the fast check runs too (`<lane>:<step>`) leaves slow CI's lane, and only
    # slow CI's.
    for name, lane in lanes.items():
        assert not {f"{name}:{s.name}" for s in lane.steps} & fast_only, name
    assert {"ui:test", "ui:prettier", "rust:fmt"} <= fast_only
    assert {"test", "prettier"} <= {s.name for s in checks.ui_lane().steps}
    assert "fmt" in {s.name for s in checks.rust_lane(fast=True).steps}
    assert "tsc" in {s.name for s in lanes["ui"].steps}
