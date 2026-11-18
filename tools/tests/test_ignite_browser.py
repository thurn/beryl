"""The browser check families' adapter: what a script reads, how results are named and
filtered, and how ui-counts are judged. Playing the UI itself is left to the families' own
runs, which need a browser."""

from __future__ import annotations

import base64
import json
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import suites
from beryl_tools.ignite import browser as adapter


def write(root: Path, files: dict[str, str]) -> None:
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def test_a_script_reads_what_it_imports_and_the_files_it_opens(tmp_path: Path) -> None:
    write(
        tmp_path,
        {
            "ui/scripts/main.mjs": (
                'import { a } from "./a.mjs";\n'
                'await import("./b.mjs");\n'
                'import { readFile } from "node:fs/promises";\n'
            ),
            "ui/scripts/a.mjs": 'import { c } from "../src/c.ts";\nexport const a = 1;\n',
            "ui/scripts/b.mjs": (
                'const data = await readFile(new URL("../verbs.json", import.meta.url));\n'
            ),
            "ui/scripts/unused.mjs": "export const u = 1;\n",
            "ui/src/c.ts": "export const c = 1;\n",
            "ui/verbs.json": "{}\n",
        },
    )
    assert adapter.script_inputs("ui/scripts/main.mjs", tmp_path) == [
        "ui/scripts/a.mjs",
        "ui/scripts/b.mjs",
        "ui/scripts/main.mjs",
        "ui/src/c.ts",
        "ui/verbs.json",
    ]


def test_the_listed_checks_of_motion_counts_and_smoke_follow_their_data() -> None:
    root = adapter.paths.REPO
    motion = adapter.list_checks("motion", root)
    verbs = suites.verb_suite()["verbs"]
    assert isinstance(verbs, list)
    assert [c["check"] for c in motion] == [f"motion:{v}/desktop" for v in verbs]
    gate = json.loads((root / adapter.COUNTS_FILE).read_text())
    assert [c["check"] for c in adapter.list_checks("ui-counts", root)] == [
        f"ui-counts:{f}" for f in gate["fixtures"]
    ]
    smoke = adapter.list_checks("ui-smoke", root)
    assert [c["check"] for c in smoke] == [
        f"ui-smoke:{adapter.kept_name(t)}" for t in suites.SMOKE_TITLES
    ]
    assert "ui/scripts/kept-harness.mjs" in smoke[0]["inputs"]


def test_a_kept_title_is_named_without_whitespace_and_read_back_exactly() -> None:
    """Ignite refuses a check name with whitespace (`CheckName` in ignite_core), and the kept
    checks are titled in sentences, so their names must hold none and still give the title.
    """
    titles = [
        "a card castable from exile is cast from its pile",
        "manual table turn Shift+Tab wraps to Untap and untaps",
        "100% of a_b\tc\u00a0d, 2/2: done",
    ]
    for title in titles:
        name = adapter.kept_name(title)
        assert not any(c.isspace() or not c.isprintable() for c in name), name
        assert adapter.kept_title(name) == title
    assert (
        adapter.kept_name(titles[0])
        == "a_card_castable_from_exile_is_cast_from_its_pile"
    )


def test_three_named_checks_report_exactly_three_results(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Each script runs only its own checks, by the names it gives them, and only the checks
    asked for reach Ignite under Ignite's names; one nobody asked for is dropped."""
    asked_of: dict[str, list[str]] = {}

    def play(
        asked: dict[str, list[str]], recorded: Path, env: dict[str, str], tmp: Path
    ) -> dict[str, dict[str, Any]]:
        asked_of.update({s: n for s, n in asked.items() if n})
        return {
            "ui-test:a land is played": {
                "check": "ui-test:a land is played",
                "outcome": "fail",
                "at": "ui/src/stage/phone.ts:212",
                "message": "page error",
            },
            "verb:fly/you/desktop": {
                "check": "verb:fly/you/desktop",
                "outcome": "pass",
            },
            "shape:pile/you/phone": {
                "check": "shape:pile/you/phone",
                "outcome": "pass",
            },
            "ui-test:not asked": {"check": "ui-test:not asked", "outcome": "pass"},
        }

    monkeypatch.setattr(adapter, "browser_env", lambda browser=None: {})
    monkeypatch.setattr(adapter, "record_fixtures", lambda into, only=(): None)
    monkeypatch.setattr(adapter, "play_scripts", play)
    results = tmp_path / "results.jsonl"
    names = [
        "ui-test:a_land_is_played",
        "ui-test:verb:fly/you/desktop",
        "ui-test:shape:pile/you/phone",
    ]
    adapter.run_checks("ui-test", names, tmp_path, results)
    assert asked_of == {
        adapter.KEPT_SCRIPT: ["a land is played"],
        adapter.VERB_SCRIPT: ["verb:fly/you/desktop"],
        adapter.SHAPE_SCRIPT: ["shape:pile/you/phone"],
    }
    lines = [json.loads(line) for line in results.read_text().splitlines()]
    assert [line["check"] for line in lines] == names
    assert lines[0] == {
        "check": "ui-test:a_land_is_played",
        "outcome": "fail",
        "at": "ui/src/stage/phone.ts:212",
        "message": "page error",
    }


def test_a_smoke_check_is_a_kept_check_under_its_own_name(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    title: str = suites.SMOKE_TITLES[0]

    def play(
        asked: dict[str, list[str]], recorded: Path, env: dict[str, str], tmp: Path
    ) -> dict[str, dict[str, Any]]:
        assert asked[adapter.KEPT_SCRIPT] == [title]
        return {f"ui-test:{title}": {"check": f"ui-test:{title}", "outcome": "pass"}}

    monkeypatch.setattr(adapter, "browser_env", lambda browser=None: {})
    monkeypatch.setattr(adapter, "record_fixtures", lambda into, only=(): None)
    monkeypatch.setattr(adapter, "play_scripts", play)
    results = tmp_path / "results.jsonl"
    name = f"ui-smoke:{adapter.kept_name(title)}"
    adapter.run_checks("ui-smoke", [name], tmp_path, results)
    assert json.loads(results.read_text()) == {
        "check": name,
        "outcome": "pass",
    }


GATE: dict[str, Any] = {
    "layouts": ["desktop", "phone"],
    "tolerance_percent": 3,
    "limits": {
        "f/desktop": {"renders": 100, "mutations": 100, "styled": 100, "laidOut": 100},
        "f/phone": {"renders": 100, "mutations": 100, "styled": 100, "laidOut": 100},
    },
}


def counts(renders: int) -> dict[str, int]:
    return {"renders": renders, "mutations": 100, "styled": 100, "laidOut": 100}


def test_counts_fail_only_past_the_tolerance_and_need_every_layout() -> None:
    within = {"f/desktop": counts(103), "f/phone": counts(90)}
    assert adapter.judge_counts("f", GATE, within) == {"outcome": "pass"}
    over = adapter.judge_counts("f", GATE, {**within, "f/phone": counts(104)})
    assert over is not None and over["outcome"] == "fail"
    assert "f/phone renders: 104 against 100" in over["message"]
    # A layout the play never measured leaves the check incomplete.
    assert adapter.judge_counts("f", GATE, {"f/desktop": counts(100)}) is None
    unlimited = adapter.judge_counts(
        "g", GATE, {"g/desktop": counts(1), "g/phone": counts(1)}
    )
    assert unlimited is not None and unlimited["outcome"] == "fail"


def test_a_place_in_a_served_module_maps_to_its_source_line() -> None:
    # Generated line 1 comes from source line 4, line 2 from none, line 3 from line 6.
    source_map = json.dumps(
        {"version": 3, "sources": ["x.ts"], "mappings": "AAGA;;AAEA"}
    )
    module = (
        "a\nb\nc\n"
        + adapter.INLINE_MAP
        + base64.b64encode(source_map.encode()).decode()
    )
    assert adapter.source_line(module, 1) == 4
    assert adapter.source_line(module, 2) is None
    assert adapter.source_line(module, 3) == 6
    assert adapter.source_line("no map", 1) is None


@pytest.mark.parametrize("kind,slow", [("tip", True), ("idle", True), ("land", False)])
def test_checks_outside_the_gate_get_long_waits(
    monkeypatch: pytest.MonkeyPatch, kind: str, slow: bool
) -> None:
    monkeypatch.setenv("IGNITE_CLASS", kind)
    monkeypatch.delenv("BERYL_LOW_PRIORITY", raising=False)
    monkeypatch.setattr(
        adapter.subprocess,
        "run",
        lambda *a, **k: adapter.subprocess.CompletedProcess([], 0, "", ""),
    )
    env = adapter.browser_env()
    assert (env.get("BERYL_LOW_PRIORITY") == "1") == slow
