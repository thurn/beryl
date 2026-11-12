import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from beryl_tools import checks, paths, runlog, selection, verdicts
from beryl_tools.selection import Coverage, Hunk, Record

NOW = datetime(2026, 11, 11, 12, tzinfo=timezone.utc)
COMBAT = "ui/src/game/combat.ts"

INTERACTIONS = """\
import assert from "node:assert/strict";

async function check(title, body, { fixture = FIXTURE } = {}) {
    if (!wanted(title)) return;
}

await check("dragging a creature forward joins the attack", async (page) => {
    await page.drag();
});

await check(
    "a blocker can be dragged onto an attacker",
    async (page) => {
        await page.drop();
    },
    { fixture: "combat-blockers" },
);

for (const size of [1180, 1600]) {
    const wide = size > 1200;
    await check(`the stack fits at ${size} wide`, async (page) => {
        assert.ok(wide || size);
    });
}
"""
UNIVERSE = [
    "ui-test:dragging a creature forward joins the attack",
    "ui-test:a blocker can be dragged onto an attacker",
    "ui-test:the stack fits at 1180 wide",
    "ui-test:the stack fits at 1600 wide",
]


def coverage_of(**checks_files: dict[str, list[list[int]]]) -> Coverage:
    """Coverage of commit `c0ffee` in which each named check ran those lines."""
    measured: dict[str, dict[str, object]] = {}
    for check, files in checks_files.items():
        measured[check] = {"files": files, "fixtures": [], "seconds": 1.0}
    return Coverage("c0ffee", NOW, measured, None)


def hunk(path: str, old: int, new: int, removed: list[str], added: list[str]) -> Hunk:
    return Hunk(path, old, len(removed), new, len(added), removed, added)


def lane_files(root: Path) -> None:
    (root / "ui" / "scripts").mkdir(parents=True)
    (root / "ui" / "scripts" / "interactions.mjs").write_text(INTERACTIONS)
    shots = {"shots": [{"name": "attack", "route": "/a"}, {"name": "block"}]}
    (root / "ui" / "shots.json").write_text(json.dumps(shots))


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A checkout holding interactions.mjs and shots.json, whose shots.json at the base had
    `attack` routed elsewhere."""
    lane_files(tmp_path)
    monkeypatch.setattr(paths, "REPO", tmp_path)
    old = {"shots": [{"name": "attack", "route": "/old"}, {"name": "block"}]}
    monkeypatch.setattr(checks, "git", lambda *args: json.dumps(old))
    return tmp_path


def test_the_diff_parses_into_each_files_hunks_with_their_text() -> None:
    diff = (
        "diff --git a/ui/src/a.ts b/ui/src/a.ts\n"
        "--- a/ui/src/a.ts\n+++ b/ui/src/a.ts\n"
        "@@ -3 +3,2 @@ fn\n-old\n+new\n+more\n"
        "@@ -9,0 +11 @@\n+added\n"
    )
    [first, second] = selection.hunks(diff)["ui/src/a.ts"]
    assert (first.old, first.old_count, first.new, first.new_count) == (3, 1, 3, 2)
    assert (first.removed, first.added) == (["old"], ["new", "more"])
    assert second.old_lines() == [9, 10]  # An insertion runs where its neighbours do.


def test_a_line_shifts_back_across_the_hunks_since_the_coverage() -> None:
    since = [hunk(COMBAT, 2, 2, ["a"], ["b", "c", "d"])]  # Two lines added at line 2.
    assert selection.shift(since, 1) == 1
    assert selection.shift(since, 3) is None  # Changed since: no line to look up.
    assert selection.shift(since, 10) == 8


def test_a_changed_line_chooses_the_checks_that_ran_it(repo: Path) -> None:
    found = coverage_of(**{"ui-test:drag": {COMBAT: [[10, 20]]}, "ui-test:hover": {}})
    found.checks["ui-test:hover"]["files"] = {COMBAT: [[30, 40]]}
    changed = {COMBAT: [hunk(COMBAT, 15, 15, ["  x();"], ["  y();"])]}
    choice = selection.choose(
        "ui-test", [COMBAT], changed, ["ui-test:drag", "ui-test:hover"], found, "c0ffee"
    )
    assert choice.reasons == {"ui-test:drag": f"coverage:{COMBAT}:15"}
    assert not choice.fill


def test_coverage_of_an_older_commit_is_read_at_the_lines_it_had(
    repo: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    found = coverage_of(**{"ui-test:drag": {COMBAT: [[10, 10]]}})
    # Since the coverage's commit, five lines were added above: line 15 was line 10.
    since = {COMBAT: [hunk(COMBAT, 1, 1, ["a"], ["a", "b", "c", "d", "e", "f"])]}
    monkeypatch.setattr(selection, "diff", lambda *args, under="": since)
    changed = {COMBAT: [hunk(COMBAT, 15, 15, ["x();"], ["y();"])]}
    choice = selection.choose(
        "ui-test", [COMBAT], changed, ["ui-test:drag"], found, "base"
    )
    assert choice.reasons == {"ui-test:drag": f"coverage:{COMBAT}:15"}


def test_comments_choose_nothing(repo: Path) -> None:
    found = coverage_of(**{"ui-test:drag": {COMBAT: [[1, 99]]}})
    changed = {COMBAT: [hunk(COMBAT, 5, 5, ["// old", ""], ["/** new", " * words */"])]}
    choice = selection.choose(
        "ui-test", [COMBAT], changed, ["ui-test:drag"], found, "c0ffee"
    )
    assert choice.idle()


def test_code_no_check_ran_fills_the_box_by_record(repo: Path) -> None:
    found = coverage_of(**{"ui-test:drag": {COMBAT: [[1, 9]]}})
    uncovered = {COMBAT: [hunk(COMBAT, 50, 50, ["a();"], ["b();"])]}
    choice = selection.choose(
        "ui-test", [COMBAT], uncovered, ["ui-test:drag"], found, "c0ffee"
    )
    assert choice.fill and not choice.reasons
    new_file = "ui/src/game/fresh.ts"
    added = {new_file: [hunk(new_file, 0, 1, [], ["export const a = 1;"])]}
    choice = selection.choose(
        "ui-test", [new_file], added, ["ui-test:drag"], found, "c0ffee"
    )
    assert choice.fill


def test_a_checks_own_body_chooses_it(repo: Path) -> None:
    script = "ui/scripts/interactions.mjs"
    # Line 8 is in the drag check's body; line 20 in the loop that sizes the stack checks.
    changed = {
        script: [
            hunk(script, 8, 8, ["    await page.drag();"], ["    await page.drag(1);"]),
            hunk(
                script, 20, 20, ["    const wide = size > 1200;"], ["    const w = 1;"]
            ),
        ]
    }
    choice = selection.choose("ui-test", [script], changed, UNIVERSE, None, "base")
    assert choice.reasons == {
        "ui-test:dragging a creature forward joins the attack": "body",
        "ui-test:the stack fits at 1180 wide": "body",
        "ui-test:the stack fits at 1600 wide": "body",
    }


def test_interactions_outside_any_check_is_the_harness(repo: Path) -> None:
    script = "ui/scripts/interactions.mjs"
    # Line 4 is in check()'s own definition: an option default reaches every check.
    changed = {script: [hunk(script, 4, 4, ["    a;"], ["    b;"])]}
    choice = selection.choose("ui-test", [script], changed, UNIVERSE, None, "base")
    assert set(choice.reasons.values()) == {"harness"}
    assert set(choice.reasons) == set(UNIVERSE)


def test_code_interactions_runs_in_node_chooses_the_checks_using_it(repo: Path) -> None:
    script = repo / "ui" / "scripts" / "interactions.mjs"
    imported = INTERACTIONS.replace(
        'import assert from "node:assert/strict";\n',
        'import assert from "node:assert/strict";\n'
        'import { type Plan, blockPlan as plan } from "../src/game/combat.ts";\n',
    )
    script.write_text(imported.replace("page.drop()", "page.drop(plan())"))
    # No page ran the line: only the check that calls it in Node can see the change.
    found = coverage_of(**{"ui-test:drag": {COMBAT: [[1, 9]]}})
    code = {COMBAT: [hunk(COMBAT, 15, 15, ["  x();"], ["  y();"])]}
    choice = selection.choose("ui-test", [COMBAT], code, UNIVERSE, found, "c0ffee")
    assert choice.reasons == {
        "ui-test:a blocker can be dragged onto an attacker": f"import:{COMBAT}"
    }
    comment = {COMBAT: [hunk(COMBAT, 15, 15, ["// x"], ["// y"])]}
    choice = selection.choose("ui-test", [COMBAT], comment, UNIVERSE, found, "c0ffee")
    assert choice.idle()
    # Called outside any check, by the harness, it reaches every check.
    wanted = "    if (!wanted(title))"
    script.write_text(imported.replace(wanted, f"    plan();\n{wanted}"))
    choice = selection.choose("ui-test", [COMBAT], code, UNIVERSE, found, "c0ffee")
    assert set(choice.reasons) == set(UNIVERSE)


def test_the_capture_harness_chooses_every_check(repo: Path) -> None:
    choice = selection.choose(
        "shots",
        ["ui/scripts/capture.mjs"],
        {},
        ["shots:attack", "shots:block"],
        None,
        "",
    )
    assert choice.reasons == {"shots:attack": "harness", "shots:block": "harness"}


def test_a_changed_shot_entry_chooses_its_shot(repo: Path) -> None:
    choice = selection.choose(
        "shots", ["ui/shots.json"], {}, ["shots:attack", "shots:block"], None, "base"
    )
    assert choice.reasons == {"shots:attack": "body"}
    # The other lane's script and list are not its body.
    assert selection.choose(
        "ui-test", ["ui/shots.json"], {}, UNIVERSE, None, "base"
    ).idle()


def test_a_rust_change_chooses_the_checks_whose_fixture_recording_changed(
    repo: Path, tmp_path: Path
) -> None:
    recorded = tmp_path / "recorded"
    recorded.mkdir()
    (recorded / "combat-declare.json").write_text('{"messages": [1]}')
    (recorded / "choice-scry.json").write_text('{"messages": [2]}')
    found = coverage_of(**{"ui-test:attack": {}, "ui-test:scry": {}})
    found.checks["ui-test:attack"]["fixtures"] = ["combat-declare"]
    found.checks["ui-test:scry"]["fixtures"] = ["choice-scry"]
    found.recordings = selection.hashes(recorded)
    (recorded / "combat-declare.json").write_text('{"messages": [3]}')
    universe = ["ui-test:attack", "ui-test:scry"]
    choice = selection.choose(
        "ui-test",
        ["rust/crates/beryl_rules/src/combat.rs"],
        {},
        universe,
        found,
        "c0ffee",
    )
    assert choice.recorded and not choice.reasons
    selection.fixture_reasons(choice, [recorded], found)
    assert choice.reasons == {"ui-test:attack": "fixture:combat-declare"}


def test_a_rust_change_without_recorded_fixtures_fills_by_record(repo: Path) -> None:
    found = coverage_of(**{"ui-test:attack": {}})
    choice = selection.choose(
        "ui-test", ["rust/x.rs"], {}, ["ui-test:attack"], found, "c0ffee"
    )
    assert choice.fill and not choice.recorded


def test_coverage_that_is_old_or_missing_falls_back(
    repo: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(selection.system, "WINDOWS", False)
    monkeypatch.setattr(checks, "git", lambda *args: "commit")
    assert "no coverage" in selection.stale(None, NOW)
    old = coverage_of()
    old.time = NOW - timedelta(days=selection.STALE_DAYS + 1)
    assert "over 3 days old" in selection.stale(old, NOW)
    assert selection.stale(coverage_of(), NOW) == ""
    # With no coverage, a source change chooses by record alone.
    changed = {COMBAT: [hunk(COMBAT, 1, 1, ["a();"], ["b();"])]}
    choice = selection.choose("ui-test", [COMBAT], changed, UNIVERSE, None, "base")
    assert choice.fill and not choice.reasons


def test_checks_run_catches_first_then_by_failure_rate_then_cheapest() -> None:
    known = {
        "a": Record(catches=0, failures=0, runs=10, seconds=1.0),
        "b": Record(catches=0, failures=0, runs=10, seconds=0.5),
        "c": Record(catches=0, failures=2, runs=10, seconds=9.0),
        "d": Record(catches=1, failures=0, runs=40, seconds=9.0),
        "e": Record(catches=0, failures=1, runs=10, seconds=9.0),
    }
    assert selection.order(known, known) == ["d", "c", "e", "b", "a"]


def test_the_box_cuts_what_does_not_fit() -> None:
    known = {c: Record(0, 0, 1, 4.0) for c in "abcd"}
    # Four seconds each across two pages: 2 s of box apiece.
    assert selection.fit(list("abcd"), known, 5.0, 2) == (["a", "b"], ["c", "d"])
    assert selection.fit(list("ab"), known, -1.0, 2) == ([], ["a", "b"])


def test_records_join_the_run_log_and_the_beads_verdicts() -> None:
    since = NOW - timedelta(days=30)
    log: list[dict[str, object]] = []
    for outcome in ("pass", "fail", "pass", "pass"):
        log.append(
            {"check": "ui-test:a", "lane": "ui-test", "outcome": outcome, "seconds": 2}
        )
    catch: dict[str, object] = {"verdict": "catch", "time": NOW.isoformat()}
    history = {"ui-test:b": [catch]}
    found = coverage_of(**{"ui-test:c": {}})
    known = selection.records(log, history, found, since)
    assert known["ui-test:a"] == Record(0, 1, 4, 2.0)
    assert known["ui-test:b"].catches == 1
    assert known["ui-test:c"].seconds == 1.0  # Timed by the coverage run alone.


def test_a_steps_results_say_why_each_check_ran_and_log_the_cut(tmp_path: Path) -> None:
    results = tmp_path / "results.jsonl"
    results.write_text(
        json.dumps({"check": "shots:attack", "outcome": "pass", "seconds": 1.0}) + "\n"
    )
    runlog.label(
        results, {"shots:attack": "body", "shots:block": "record"}, ["shots:block"]
    )
    run = runlog.Run("check")
    records = run.records("shots", "ui-shots", True, 3.0, results)
    assert [(r["check"], r["outcome"], r["chosen"]) for r in records] == [
        ("shots:attack", "pass", "body"),
        ("shots:block", "skip", "cut"),
    ]


VERBS = {
    "verbs": ["fly", "emerge"],
    "layouts": ["desktop"],
    "moment": {
        "yours": {"whole": "combat-declare", "cut": "verbs-cast-stops"},
        "theirs": {"whole": "verbs-cast-theirs", "cut": "verbs-cast-stops"},
    },
    "more": {"zones": {"yours": "verbs-zones", "theirs": "verbs-zones-theirs"}},
    "watches": {"fly": ["zones"]},
}
VERB_CHECKS = [
    "verb:fly/yours/desktop",
    "verb:fly/theirs/desktop",
    "verb:emerge/yours/desktop",
    "verb:emerge/theirs/desktop",
]


def test_the_verb_suite_lists_its_checks_and_fixtures(repo: Path) -> None:
    assert selection.verb_checks() == []
    assert selection.verb_fixtures() == []
    (repo / "ui" / "verbs.json").write_text(json.dumps(VERBS))
    assert selection.verb_checks() == VERB_CHECKS
    assert selection.verb_fixtures() == [
        "combat-declare",
        "verbs-cast-stops",
        "verbs-cast-theirs",
        "verbs-zones",
        "verbs-zones-theirs",
    ]
    # They are the lane's checks beside those interactions.mjs gives.
    found = selection.lane_checks("ui-test", [], None)
    assert found[-4:] == VERB_CHECKS
    assert "ui-test:dragging a creature forward joins the attack" in found


def test_a_change_to_the_verb_suite_chooses_its_checks(repo: Path) -> None:
    universe = [*UNIVERSE, *VERB_CHECKS]
    script = "ui/scripts/verbs.mjs"
    # A new file, which git gives no hunks for.
    choice = selection.choose("ui-test", [script], {}, universe, None, "")
    assert choice.reasons == dict.fromkeys(VERB_CHECKS, "body")
    code = {script: [hunk(script, 9, 9, ["a"], ["b"])]}
    choice = selection.choose("ui-test", [script], code, universe, None, "")
    assert choice.reasons == dict.fromkeys(VERB_CHECKS, "body")
    comment = {script: [hunk(script, 9, 9, ["// a"], ["// b"])]}
    choice = selection.choose("ui-test", [script], comment, universe, None, "")
    assert choice.reasons == {}
    choice = selection.choose("ui-test", ["ui/verbs.json"], {}, universe, None, "")
    assert choice.reasons == dict.fromkeys(VERB_CHECKS, "body")
    # The shots have none of them.
    choice = selection.choose("shots", [script], {}, ["shots:attack"], None, "")
    assert choice.reasons == {}


def test_each_name_goes_to_the_script_whose_check_it_is() -> None:
    every = {
        selection.INTERACTIONS: [],
        selection.VERB_SCRIPT: [],
        selection.SHAPE_SCRIPT: [],
    }
    assert selection.scripts([]) == every
    names = ["joins", "verb:fly/yours/desktop", "shape:pick/yours/phone", "drag"]
    assert selection.scripts(names) == {
        selection.INTERACTIONS: ["joins", "drag"],
        selection.VERB_SCRIPT: ["verb:fly/yours/desktop"],
        selection.SHAPE_SCRIPT: ["shape:pick/yours/phone"],
    }
    assert selection.scripts(["verb:emerge"]) == {
        selection.VERB_SCRIPT: ["verb:emerge"]
    }
    assert selection.scripts(["shape:"]) == {selection.SHAPE_SCRIPT: ["shape:"]}
    assert selection.scripts(["drag"]) == {selection.INTERACTIONS: ["drag"]}


SHAPES = {
    "shapes": {"pick": {"yours": "shape-pick", "theirs": "shape-pick-theirs"}},
    "layouts": ["desktop", "phone"],
}
SHAPE_CHECKS = [
    "shape:pick/yours/desktop",
    "shape:pick/yours/phone",
    "shape:pick/theirs/desktop",
    "shape:pick/theirs/phone",
]


def test_the_shape_suite_lists_its_checks_and_fixtures(repo: Path) -> None:
    assert selection.shape_checks() == []
    assert selection.shape_fixtures() == []
    (repo / "ui" / "shapes.json").write_text(json.dumps(SHAPES))
    assert selection.shape_checks() == SHAPE_CHECKS
    assert selection.shape_fixtures() == ["shape-pick", "shape-pick-theirs"]
    # They are the lane's checks, after every other.
    assert selection.lane_checks("ui-test", [], None)[-4:] == SHAPE_CHECKS


def test_a_change_to_the_shape_suite_chooses_its_checks(repo: Path) -> None:
    universe = [*UNIVERSE, *VERB_CHECKS, *SHAPE_CHECKS]
    script = "ui/scripts/shapes.mjs"
    code = {script: [hunk(script, 9, 9, ["a"], ["b"])]}
    choice = selection.choose("ui-test", [script], code, universe, None, "")
    assert choice.reasons == dict.fromkeys(SHAPE_CHECKS, "body")
    choice = selection.choose("ui-test", ["ui/shapes.json"], {}, universe, None, "")
    assert choice.reasons == dict.fromkeys(SHAPE_CHECKS, "body")
    for cases in selection.SHAPE_CASES:
        code = {cases: [hunk(cases, 9, 9, ["a"], ["b"])]}
        choice = selection.choose("ui-test", [cases], code, universe, None, "")
        assert choice.reasons == dict.fromkeys(SHAPE_CHECKS, "body")
    # A change to the verb suite leaves them be.
    choice = selection.choose("ui-test", ["ui/verbs.json"], {}, universe, None, "")
    assert choice.reasons == dict.fromkeys(VERB_CHECKS, "body")


def test_checks_the_prune_moved_to_slow_ci_are_never_chosen(
    repo: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    moved = verdicts.SlowOnly("shots:block", "2026-11-11", 9.0, 30)
    monkeypatch.setattr(verdicts, "SLOW_ONLY", (moved,))
    assert selection.lane_checks("shots", [], None) == ["shots:attack"]
