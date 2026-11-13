import base64
import json
import subprocess
from pathlib import Path

import pytest

from beryl_tools import coverage, paths, system


def inline(text: str, sources: list[str], mappings: str, comment: str = "//") -> str:
    """`text` with an inline source map, as Vite serves a module (`//`) or a stylesheet."""
    source_map = {"version": 3, "sources": sources, "mappings": mappings}
    encoded = base64.b64encode(json.dumps(source_map).encode()).decode()
    url = f"# sourceMappingURL=data:application/json;base64,{encoded}"
    return text + (f"//{url}" if comment == "//" else f"/*{url} */")


# Three generated lines, each mapped from its column 0 to lines 2, 5 and 9 of the one source:
# each segment's source line is relative to the previous one's (+1, +3, +4, 0-based).
THREE_LINES = "AACA;AAGA;AAIA"


def test_vlq_decodes_signs_and_continuations() -> None:
    assert coverage.vlq("AAAA") == [0, 0, 0, 0]
    assert coverage.vlq("CDE") == [1, -1, 2]
    assert coverage.vlq("gB") == [16]


def test_flatten_gives_each_offset_its_innermost_count() -> None:
    nested = [[0, 100, 1], [20, 30, 2], [10, 50, 0]]
    assert coverage.flatten(nested) == [
        (0, 10, 1),
        (10, 20, 0),
        (20, 30, 2),
        (30, 50, 0),
        (50, 100, 1),
    ]


def test_a_module_maps_the_blocks_that_ran_to_its_source_lines(tmp_path: Path) -> None:
    text = inline("a();\nb();\nc();\n", ["combat.ts"], THREE_LINES)
    module = coverage.Mapped(
        text,
        coverage.source_base({"url": "/src/game/combat.js"}, tmp_path),
        tmp_path,
    )
    # The module ran, but not the block of its second line (offsets 5 to 10).
    ran = module.covered([[0, len(text), 1], [5, 10, 0]])
    assert ran == {("ui/src/game/combat.ts", 2), ("ui/src/game/combat.ts", 9)}


def test_offsets_count_utf16_units_as_v8_does(tmp_path: Path) -> None:
    # An emoji is one character to Python but two UTF-16 units to V8: the second line starts at
    # offset 9, not 8.
    text = inline("a('😀');\nb();\nc();\n", ["x.ts"], THREE_LINES)
    module = coverage.Mapped(text, tmp_path / "ui" / "src", tmp_path)
    ran = module.covered([[0, len(text) + 1, 1], [9, 14, 0]])
    assert sorted(line for _, line in ran) == [2, 9]


def test_a_stylesheet_names_files_on_disk_and_skips_node_modules(
    tmp_path: Path,
) -> None:
    sheet = tmp_path / "ui" / "src" / "screens" / "screens.css"
    font = tmp_path / "ui" / "node_modules" / "font" / "wght.css"
    # Two sources: the first segment from screens.css's line 1, the next from the font's.
    text = inline(".a{}\n.b{}\n", [str(sheet), str(font)], "AAAA;ACAA", comment="/*")
    mapped = coverage.Mapped(
        text, coverage.source_base({"file": str(sheet)}, tmp_path), tmp_path
    )
    assert mapped.covered([[0, 9, 1]]) == {("ui/src/screens/screens.css", 1)}


def test_text_without_a_source_map_maps_nothing(tmp_path: Path) -> None:
    assert coverage.Mapped("a();\n", tmp_path, tmp_path).covered([[0, 4, 1]]) == set()


def write_raw(raw: Path, root: Path) -> None:
    """The records ui/scripts/capture.mjs writes for three checks: one that ran a module's
    first and last lines, one that failed with only the first, one whose coverage failed.
    """
    text = inline("a();\nb();\nc();\n", ["combat.ts"], THREE_LINES)
    (raw / "sources").mkdir(parents=True)
    (raw / "sources" / "m.json").write_text(
        json.dumps({"url": "/src/game/combat.js", "text": text})
    )
    module = {"source": "m.json"}
    records = [
        {
            "check": "ui-test:declares an attack",
            "outcome": "pass",
            "seconds": 1.5,
            "fixtures": ["combat-declare"],
            "js": [{**module, "ranges": [[0, len(text), 1], [5, 10, 0]]}],
            "css": [],
        },
        {
            "check": "shots:combat",
            "outcome": "fail",
            "seconds": 2.0,
            "fixtures": [],
            "js": [{**module, "ranges": [[0, len(text), 1], [5, len(text), 0]]}],
            "css": [],
        },
        {"check": "shots:menu", "outcome": "pass", "seconds": 0.5},
    ]
    (raw / "checks-1.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))


def test_the_coverage_file_maps_check_ids_to_line_runs(tmp_path: Path) -> None:
    raw = tmp_path / "raw"
    write_raw(raw, tmp_path)
    checks = coverage.build(raw, tmp_path)
    path = coverage.write(checks, "abc123", False, {"ui-test": 0, "shots": 1}, 9.0)
    document = json.loads(path.read_text())
    assert path == coverage.folder() / "abc123.json"
    assert document["commit"] == "abc123"
    assert document["failed"] == ["shots:combat"]
    assert document["lanes"] == {"ui-test": 0, "shots": 1}
    assert document["checks"] == {
        "ui-test:declares an attack": {
            "outcome": "pass",
            "seconds": 1.5,
            "fixtures": ["combat-declare"],
            "files": {"ui/src/game/combat.ts": [[2, 2], [9, 9]]},
        },
        "shots:combat": {
            "outcome": "fail",
            "seconds": 2.0,
            "fixtures": [],
            "files": {"ui/src/game/combat.ts": [[2, 2]]},
        },
        "shots:menu": {
            "outcome": "pass",
            "seconds": 0.5,
            "fixtures": [],
            "files": None,
        },
    }
    latest = json.loads((coverage.folder() / "latest.json").read_text())
    assert latest["commit"] == "abc123" and latest["file"] == "abc123.json"


def test_a_dirty_tree_leaves_latest_alone(tmp_path: Path) -> None:
    coverage.write({}, "abc123", False, {}, 1.0)
    path = coverage.write({}, "def456", True, {}, 1.0)
    assert path.name == "def456-dirty.json"
    latest = json.loads((coverage.folder() / "latest.json").read_text())
    assert latest["commit"] == "abc123"


def started(monkeypatch: pytest.MonkeyPatch) -> list[list[str]]:
    """The commands `coverage.refresh` starts, recorded instead of run, from a main checkout
    at /main."""
    commands: list[list[str]] = []
    monkeypatch.setattr(paths, "main_checkout", lambda: Path("/main"))

    def popen(command: list[str], **_: object) -> None:
        commands.append(command)

    monkeypatch.setattr(subprocess, "Popen", popen)
    return commands


def test_a_refresh_measures_a_commit_newer_than_the_coverage(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    commands = started(monkeypatch)
    coverage.write({}, "abc123", False, {}, 1.0)
    line = coverage.refresh("def456")
    assert "measuring def456" in line
    [command] = commands
    assert command[-4:] == ["by", "coverage", "--commit", "def456"]


def test_a_refresh_skips_the_commit_coverage_has(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    commands = started(monkeypatch)
    coverage.write({}, "abc123", False, {}, 1.0)
    assert "measured already" in coverage.refresh("abc123")
    assert commands == []


def test_a_refresh_waits_for_no_coverage_run_in_progress(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    commands = started(monkeypatch)
    with system.file_lock(coverage.folder() / ".lock"):
        assert "in progress" in coverage.refresh("def456")
    assert commands == []


def test_spans_join_consecutive_lines() -> None:
    assert coverage.spans([5, 1, 2, 3, 9, 8]) == [[1, 3], [5, 5], [8, 9]]
