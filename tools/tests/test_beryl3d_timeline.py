"""`just b3d timeline-parity`: comparing the two clients' timelines and the allowlist."""

from __future__ import annotations

import argparse
import json
import os
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
        "sounds": [],
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


def test_a_step_whose_sound_alone_differs_counts() -> None:
    # The web interface picks a step's sounds in audio/steps.ts and Beryl 3D in its port,
    # sound.rs: a step whose sound differs while its timing agrees is one differing step.
    web = [step(sounds=[["cast", "start", None]]), step(100.0, number=1)]
    native = [step(sounds=[["place", "end", None]]), step(100.0, number=1)]
    found = timeline.compare(web, native)
    assert found is not None and found.count == 1
    assert found.web is not None and found.web["sounds"] == [["cast", "start", None]]
    hit = [["hit", "start", 0.376]]
    assert timeline.compare([step(sounds=hit)], [step(sounds=hit)]) is None


def test_a_beat_the_web_holds_for_cards_closing_up_is_accepted() -> None:
    # The web's beat 1 lasts until cards closing up after the vanished card are in place,
    # 40 ms (0.4 of its 100 ms) after its last step ends; Beryl 3D's table closes up at
    # once. Beat 2 and the step after it follow 40 ms later on the web, and are accepted.
    native = [
        step(0.0, verb="vanish"),
        step(100.0, beat=1, verb="vanish"),
        step(200.0, beat=2, verb="flip"),
        step(200.0, beat=2, number=1, verb="flip"),
    ]
    web = [dict(s) for s in native]
    for later in web[2:]:
        later["start"] += 40.0
        later["end"] += 40.0
    assert timeline.compare(web, native) is None


def test_a_later_beat_that_no_close_up_explains_counts() -> None:
    # No card left in beat 0, so nothing closes up after it: beat 1 beginning later on the
    # web counts. After a vanish, a beat later by more than its close-up counts too.
    native = [step(0.0), step(100.0, beat=1)]
    found = timeline.compare([step(0.0), step(140.0, beat=1)], native)
    assert found is not None and found.count == 1
    native = [step(0.0, verb="vanish"), step(100.0, beat=1)]
    web = [step(0.0, verb="vanish"), step(141.5, beat=1)]
    found = timeline.compare(web, native)
    assert found is not None and found.count == 1


def test_a_beat_after_a_cut_waits_for_the_sessions_close_up() -> None:
    # A finished ability vanishes in no time; the session gives its close-up's length.
    cut = step(100.0, number=1, verb="vanish")
    cut["start"] = cut["end"] = 100.0
    native = [step(0.0), cut, step(100.0, beat=1)]
    web = [dict(s) for s in native]
    web[2] = step(100.0 + timeline._cut_closes(), beat=1)
    assert timeline.compare(web, native) is None


def test_a_beat_that_begins_before_the_last_one_ended_on_the_web_is_not_accepted() -> (
    None
):
    # A close-up only makes a beat last longer: the web's beat 1 begins 40 ms later than
    # Beryl 3D's but 60 ms before its own beat 0 ended, which no close-up explains.
    native = [step(0.0), step(0.0, beat=1)]
    web = [step(0.0), step(40.0, beat=1)]
    found = timeline.compare(web, native)
    assert found is not None and found.count == 1


def test_a_strike_the_web_holds_back_is_accepted_with_the_steps_after_it() -> None:
    # The second strike of the beat would cross the first's on the web, which holds it and
    # the life loss after it back by three 25 ms waits.
    strike, unblocked = "strike", "unblocked attacker"
    count, loss = "count", "life loss"
    native = [
        step(0.0, verb=strike, case=unblocked),
        step(470.0, number=1, verb=count, case=loss),
        step(470.0, number=2, verb=strike, case=unblocked),
        step(940.0, number=3, verb=count, case=loss),
    ]
    web = [dict(s) for s in native]
    for later in web[2:]:
        later["start"] += 75.0
        later["end"] += 75.0
    assert timeline.compare(web, native) is None


def test_a_step_held_back_that_is_no_strike_or_not_by_whole_waits_counts() -> None:
    native = [step(0.0), step(100.0, number=1, verb="strike")]
    held = [step(0.0), step(110.0, number=1, verb="strike")]
    found = timeline.compare(held, native)
    assert found is not None and found.count == 1
    flown = [step(0.0), step(100.0, number=1)]
    found = timeline.compare([step(0.0), step(125.0, number=1)], flown)
    assert found is not None and found.count == 1


def test_a_shift_that_is_not_accepted_counts_every_step_it_moves() -> None:
    # The first step is 30 ms late for no reason the comparison accepts, so the steps
    # it pushes later in its beat each count too.
    native = [step(0.0, beat=1), step(100.0, beat=1, number=1)]
    web = [step(30.0, beat=1), step(130.0, beat=1, number=1)]
    found = timeline.compare(web, native)
    assert found is not None and found.count == 2


def test_a_later_step_of_a_different_length_is_not_accepted() -> None:
    native = [step(0.0), step(100.0, beat=1)]
    longer = step(240.0, beat=1)
    longer["end"] = 400.0
    found = timeline.compare([step(0.0), longer], native)
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


def test_parity_reuses_the_binary_built_and_leaves_no_files(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    builds: list[list[str]] = []
    line = json.dumps(step())
    native: Path = tmp_path / "native.jsonl"
    native.write_text(line + "\n")
    exported: Path = tmp_path / "exported"
    exported.mkdir()

    def build(arguments: list[str]) -> int:
        builds.append(arguments)
        return 0

    def run(command: list[str], **_options: Any) -> subprocess.CompletedProcess[str]:
        if command == [str(tmp_path / "record"), "--all"]:
            return subprocess.CompletedProcess[str](command, 0, "recordings.bin\n", "")
        if command == [str(tmp_path / "b3d"), "export", str(exported)]:
            (exported / "r.you.json").write_text("{}")
            return subprocess.CompletedProcess[str](command, 0, "", "")
        assert command == [str(tmp_path / "b3d"), "timeline", "--all"]
        return subprocess.CompletedProcess[str](command, 0, f"command\n{native}\n", "")

    monkeypatch.setattr(cli, "build", build)
    monkeypatch.setattr(cli, "program", lambda name: tmp_path / name)
    monkeypatch.setattr(cli, "linked", lambda name: tmp_path / name)
    monkeypatch.setattr(timeline, "out", run)
    monkeypatch.setattr(timeline.tempfile, "mkdtemp", lambda prefix: str(exported))
    monkeypatch.setattr(timeline, "web_lines", lambda pack, directory: [line])
    monkeypatch.setattr(timeline, "load_allowlist", dict)

    assert (
        timeline.run_parity(
            argparse.Namespace(
                prefix="", fixtures=False, update=False, refresh_web=False
            )
        )
        == 0
    )
    assert builds == [cli.SHARED_BUILD]
    # Each run's exported recordings and Beryl 3D's timeline file once filled the disk.
    assert not exported.exists() and not native.exists()


def test_pruning_the_web_cache_keeps_only_recent_files(tmp_path: Path) -> None:
    now = 1_000_000_000.0
    old = tmp_path / "web-timelines-old.jsonl"
    recent = tmp_path / "web-timelines-recent.jsonl"
    other = tmp_path / "other.jsonl"
    partial = tmp_path / "web-timelines-killed.jsonl.123.tmp"
    for file in (old, recent, other, partial):
        file.write_text("")
        os.utime(file, (now - 3 * 86400, now - 3 * 86400))
    os.utime(recent, (now - 3600, now - 3600))
    timeline.prune_web_cache(tmp_path, now)
    assert not old.exists() and recent.exists() and other.exists()
    assert not partial.exists()


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
    exported: Path = tmp_path / "exported"
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
