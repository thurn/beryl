"""`just b3d timeline-parity`: comparing the two clients' timelines and the allowlist."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path
from typing import Any

import pytest
from beryl_tools.beryl3d import cli, timeline


def step(
    start: float = 0.0, beat: int = 0, number: int = 0, **extra: Any
) -> dict[str, Any]:
    return {
        "recording": "r",
        "seat": "you",
        "view": 2,
        "beat": beat,
        "step": number,
        "verb": "fly",
        "case": None,
        "start": start,
        "end": start + 100.0,
        "effects": [],
        **extra,
    }


def test_equal_timelines_agree() -> None:
    steps = [step(), step(100.0, number=1)]
    assert timeline.compare(steps, [dict(s) for s in steps]) is None


def test_two_milliseconds_apart_is_one_step_and_half_a_millisecond_is_none() -> None:
    assert timeline.compare([step(0.0)], [step(0.5)]) is None
    found = timeline.compare(
        [step(0.0), step(5.0, number=1)], [step(2.0), step(5.0, number=1)]
    )
    assert found is not None and found.count == 1


def test_a_step_one_side_lacks_counts_once() -> None:
    found = timeline.compare([step(), step(100.0, number=1)], [step()])
    assert found is not None and found.count == 1
    assert found.native is None and found.web is not None


def test_a_step_counts_once_however_many_fields_differ() -> None:
    found = timeline.compare([step()], [step(7.0, verb="tap", effects=["settle@end"])])
    assert found is not None and found.count == 1


def test_an_unlisted_differing_recording_fails() -> None:
    web = {"r/you": [step()]}
    native = {"r/you": [step(9.0)]}
    found = timeline.differences(web, native, {"r/you"})
    assert [d.key for d in timeline.over_limit(found, {})] == ["r/you"]
    assert timeline.over_limit(found, {"r/you": 1}) == []
    assert timeline.over_limit(found, {"r/you": 0}) != []


def test_update_lowers_an_entry_and_refuses_to_raise_one() -> None:
    web = {"a/you": [step(), step(1.0, number=1)], "b/you": [step()]}
    native = {"a/you": [step()], "b/you": [step(9.0)]}
    scope = {"a/you", "b/you"}
    found = timeline.differences(web, native, scope)
    allowed = {"a/you": 3, "b/you": 0, "outside/you": 4}
    entries, refused = timeline.lowered(found, allowed, scope, raise_=False)
    assert entries == {"a/you": 1, "outside/you": 4}
    assert refused == ["b/you"]
    entries, refused = timeline.lowered(found, allowed, scope, raise_=True)
    assert entries["b/you"] == 1 and refused == []


def test_parity_reuses_the_binary_built_with_the_pack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    builds: list[list[str]] = []
    line = json.dumps(step())
    native: Path = tmp_path / "native.jsonl"
    native.write_text(line + "\n")

    def build(arguments: list[str]) -> int:
        builds.append(arguments)
        return 0

    def run(command: list[str], **_options: Any) -> subprocess.CompletedProcess[str]:
        if command == [str(tmp_path / "record")]:
            return subprocess.CompletedProcess[str](command, 0, "recordings.bin\n", "")
        if command == [str(tmp_path / "b3d"), "export", str(tmp_path)]:
            (tmp_path / "r.you.json").write_text("{}")
            return subprocess.CompletedProcess[str](command, 0, "", "")
        assert command == [str(tmp_path / "b3d"), "timeline", "--all"]
        return subprocess.CompletedProcess[str](command, 0, f"command\n{native}\n", "")

    monkeypatch.setattr(cli, "build", build)
    monkeypatch.setattr(cli, "program", lambda name: tmp_path / name)
    monkeypatch.setattr(cli, "linked", lambda name: tmp_path / name)
    monkeypatch.setattr(timeline, "out", run)
    monkeypatch.setattr(timeline.tempfile, "mkdtemp", lambda prefix: str(tmp_path))
    monkeypatch.setattr(timeline, "web_lines", lambda pack, directory: [line])
    monkeypatch.setattr(timeline, "load_allowlist", dict)

    assert (
        timeline.run_parity(argparse.Namespace(prefix="", fixtures=False, update=False))
        == 0
    )
    assert builds == [cli.SHARED_BUILD]


def test_web_hash_moves_with_every_file_the_web_timeline_imports(
    tmp_path: Path,
) -> None:
    # The cached web timelines must go stale when any module the script loads changes: a
    # module of src/ outside the stage, a TSX file served as its constants and JSON data
    # outside ui/. A file nothing imports, or one a TSX file imports, leaves the cache alone.
    ui = tmp_path / "ui"
    (ui / "scripts").mkdir(parents=True)
    (ui / "src" / "stage").mkdir(parents=True)
    (ui / "src" / "components").mkdir()
    (tmp_path / "data").mkdir()
    exported = tmp_path / "exported"
    exported.mkdir()
    files = {
        ui / timeline.WEB_ENTRY: 'const { s } = await import("../src/stage/stage.ts");',
        ui
        / "src/stage/stage.ts": 'import { r } from "../records";\nexport * from "./timing";',
        ui
        / "src/stage/timing.ts": 'import c from "../../../data/conventions.json" with { type: "json" };\nimport { W } from "../components/Card";',
        ui / "src/records.ts": "export const r = 1;",
        ui / "src/components/Card.tsx": 'import "../browser";\nexport const W = 2;',
        ui / "src/browser.ts": "window;",
        ui / "src/unused.ts": "export const u = 3;",
        tmp_path / "data/conventions.json": "{}",
    }
    for file, text in files.items():
        file.write_text(text)
    before = timeline.web_hash("pack", ui, exported)
    for file in [
        ui / "src/records.ts",
        ui / "src/stage/timing.ts",
        ui / "src/components/Card.tsx",
        tmp_path / "data/conventions.json",
    ]:
        file.write_text(file.read_text() + "\n// changed")
        assert timeline.web_hash("pack", ui, exported) != before, file
        before = timeline.web_hash("pack", ui, exported)
    for file in [ui / "src/unused.ts", ui / "src/browser.ts"]:
        file.write_text(file.read_text() + "\n// changed")
        assert timeline.web_hash("pack", ui, exported) == before, file
