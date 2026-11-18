"""`just b3d parity`: the parity list's counts, its printed lines and `--check`, on small values
built by hand."""

import json
import re
from pathlib import Path
from typing import Any

from beryl_tools.beryl3d import parity

WINDOWS = ("1600x900", "393x759")


def result_row(recording: str, seat: str, window: str, outcome: str) -> dict[str, str]:
    return {"test": f"b3d:{recording}/{seat}/{window}", "outcome": outcome}


def results(
    failing: tuple[str, ...] = (), recordings: tuple[str, ...] = ()
) -> parity.Results:
    """Every recording played from both seats in both layouts; a test of each of `failing`
    fails."""
    found = parity.Results()
    for phone, window in ((False, WINDOWS[0]), (True, WINDOWS[1])):
        rows = [
            result_row(r, seat, window, "fail" if r in failing else "pass")
            for r in recordings
            for seat in ("you", "opponent")
        ]
        found.add({"results": rows}, phone)
    return found


def conventions_file() -> dict[str, Any]:
    return {
        "entries": [
            {"kind": "exception", "name": "Phase icons"},
            {"kind": "settled", "name": "Hand fan"},
        ],
        "verbs": [{"name": "fly"}],
        "effects": [{"name": "glow"}],
        "marks": [{"name": "eligible"}],
    }


def parity_file() -> dict[str, Any]:
    return {
        "tools": [
            {"job": "specs", "web": "", "skills": ["specs"], "b3d": "just b3d tree"},
            {"job": "bugfix", "web": "", "skills": ["bugfix"], "b3d": None},
        ],
        "skills": {"specs": "specs", "bugfix": "bugfix", "cifix": "none: no client"},
        "web_screens": {
            "ui/src/screens/Game.tsx": "game",
            "ui/src/screens/Scry.tsx": "none: static mock-up",
        },
        "verbs": {"fly": "verbs-zones"},
        "effects": {"glow": "verbs-combat"},
        "marks": {"eligible": "first-target"},
    }


def screens_file() -> dict[str, Any]:
    return {
        "screens": [
            {"name": "game", "built": True, "recording": "screen/start"},
            {"name": "testing", "built": False, "recording": "screen/start"},
        ],
        "prompt_shapes": {"pick": "shape-pick"},
    }


RECORDINGS = (
    "verbs-zones",
    "verbs-combat",
    "first-target",
    "shape-pick",
    "counterspell/cast",
    "screen/start",
)


def sources(
    failing: tuple[str, ...] = (), timeline: tuple[str, ...] = ()
) -> parity.Sources:
    return parity.Sources(
        results=results(failing, RECORDINGS),
        fixtures=["verbs-zones", "verbs-combat", "first-target", "shape-pick"],
        parity=parity_file(),
        screens=screens_file(),
        ui_conventions=conventions_file(),
        conventions={
            "exception": {"Phase icons": {"recording": "verbs-zones"}},
            "settled": {"Hand fan": {"open": "needs a numbered exception"}},
        },
        timeline_keys=set(timeline),
        listed_faults=set(),
    )


def kinds(found: parity.Sources) -> dict[str, parity.Kind]:
    return {k.kind: k for k in parity.report(found)}


def test_a_recording_with_one_failing_test_is_not_done() -> None:
    found = kinds(sources(failing=("verbs-combat",)))
    assert found["fixtures"].done == 3 and found["fixtures"].total == 4
    assert found["fixtures"].missing == ["verbs-combat"]
    # The effect named for the failing recording is not done either.
    assert found["effects"].done == 0


def test_a_recording_played_in_one_layout_only_is_not_done() -> None:
    found = sources()
    del found.results.phone["shape-pick"]
    assert kinds(found)["prompt shapes"].missing == ["pick"]


def test_a_recording_listed_in_timeline_parity_is_not_done() -> None:
    found = kinds(sources(timeline=("counterspell/cast/opponent",)))
    assert found["card specs"].missing == ["counterspell/cast"]
    timelines = found["timelines"]
    # Two seats of the four fixtures and the one spec moment; the screen recording is neither.
    assert (timelines.done, timelines.total) == (9, 10)
    assert timelines.missing == ["counterspell/cast/opponent"]


def test_a_verb_whose_recording_is_done_is_done() -> None:
    found = kinds(sources(failing=("verbs-combat",)))
    assert (found["verbs"].done, found["verbs"].total) == (1, 1)
    assert (found["marks"].done, found["marks"].total) == (1, 1)


def test_screens_conventions_and_tools_are_counted_from_their_files() -> None:
    found = kinds(sources())
    assert found["screens"].missing == ["testing"]
    assert found["conventions"].missing == [
        "Hand fan (open: needs a numbered exception)"
    ]
    assert found["tools"].missing == ["bugfix"]


def test_a_screen_or_convention_whose_recording_fails_is_not_done() -> None:
    found = kinds(sources(failing=("screen/start", "verbs-zones")))
    assert found["screens"].missing == ["game", "testing"]
    assert found["conventions"].missing == [
        "Phase icons",
        "Hand fan (open: needs a numbered exception)",
    ]


def test_the_printed_lines_follow_the_docs_format() -> None:
    missing = [f"moment-{n}" for n in range(8)]
    line = parity.Kind("card specs", 1388, 1396, missing).line(full=False)
    assert line == (
        "card specs    1,388 / 1,396   missing: moment-0, moment-1, moment-2, moment-3,"
        " moment-4 (+3 more, --full)"
    )
    assert "(+" not in parity.Kind("card specs", 0, 8, missing).line(full=True)
    assert parity.Kind("verbs", 16, 16, []).line(full=False) == "verbs         16 / 16"
    shape = re.compile(r"^[a-z ]{13} [\d,]+ / [\d,]+( {3}.+)?$")
    for kind in parity.report(sources(failing=("verbs-combat",))):
        assert shape.match(kind.line(full=False)), kind.line(full=False)


def check(skills: list[str], web_files: list[str]) -> list[str]:
    found = sources()
    return parity.problems(
        found.parity,
        found.screens,
        found.ui_conventions,
        found.conventions,
        skills,
        web_files,
    )


WEB_FILES = ["ui/src/screens/Game.tsx", "ui/src/screens/Scry.tsx"]


def test_check_passes_when_every_skill_and_screen_file_is_mapped() -> None:
    assert check(["bugfix", "cifix", "specs"], WEB_FILES) == []


def test_check_fails_for_a_skill_directory_missing_from_the_skills() -> None:
    assert check(["bugfix", "cifix", "new-skill", "specs"], WEB_FILES) == [
        "parity.json skills: new-skill has no entry"
    ]


def test_check_fails_for_a_web_screen_file_missing_from_the_web_screens() -> None:
    found = check(["bugfix", "cifix", "specs"], [*WEB_FILES, "ui/src/screens/New.tsx"])
    assert found == ["parity.json web_screens: ui/src/screens/New.tsx has no entry"]


def test_check_fails_for_a_key_that_no_longer_exists() -> None:
    found = check(["bugfix", "specs"], WEB_FILES[:1])
    assert "parity.json skills: cifix no longer exists" in found
    assert "parity.json web_screens: ui/src/screens/Scry.tsx no longer exists" in found


def test_web_screen_files_are_the_screens_views_openings_and_lobby(
    tmp_path: Path,
) -> None:
    for name in (
        "ui/src/screens/Game.tsx",
        "ui/src/screens/glide.ts",
        "ui/src/table/TablePromptView.tsx",
        "ui/src/table/TableOpening.tsx",
        "ui/src/table/TableViewer.tsx",
        "ui/src/table/lobby/DeckPicker.tsx",
    ):
        (tmp_path / name).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / name).write_text("")
    assert parity.web_screen_files(tmp_path) == [
        "ui/src/screens/Game.tsx",
        "ui/src/table/TableOpening.tsx",
        "ui/src/table/TablePromptView.tsx",
        "ui/src/table/lobby/DeckPicker.tsx",
    ]


def test_results_are_read_from_a_folder(tmp_path: Path) -> None:
    for name, window in (("desktop.json", WINDOWS[0]), ("phone.json", WINDOWS[1])):
        rows = [result_row("counterspell/cast", "you", window, "pass")]
        report = {"results": rows}
        (tmp_path / name).write_text(f"building\n{json.dumps(report)}\n")
    found = parity.read_results(tmp_path)
    assert found.passes("counterspell/cast")
    assert found.desktop == {"counterspell/cast": {"you"}}
