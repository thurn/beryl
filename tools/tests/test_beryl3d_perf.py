"""`just b3d perf`: the line printed for a fixture and the late frames grouped by verb, on
frame logs written by hand."""

import argparse
import json
from pathlib import Path

import pytest

from beryl_tools.beryl3d import app, perf


def late(at: float, gap: float, client: float, verbs: list[str]) -> dict[str, object]:
    return {
        "at_ms": at,
        "since_last_ms": gap,
        "waited_ms": 0.0,
        "client_ms": client,
        "encoded_ms": client / 2,
        "gpu_ms": None,
        "verbs": verbs,
        "searching": False,
        "held_to_60": False,
        "hz": 60.0,
    }


def write(path: Path, frames: list[dict[str, object]], summary: bool = True) -> Path:
    lines: list[object] = [{"run": {"fixture": "verbs-zones"}}]
    lines += [{"late": frame} for frame in frames]
    if summary:
        lines.append(
            {"summary": {"frames": 1200, "late": len(frames), "worst_wait_ms": 4.5}}
        )
    path.write_text("\n".join(json.dumps(line) for line in lines) + "\n")
    return path


FRAMES: list[dict[str, object]] = [
    late(100, 20.0, 1.0, ["fly"]),
    late(200, 36.0, 3.0, ["fly", "tap"]),
    late(300, 25.0, 2.0, []),
]


def test_a_fixtures_line_gives_its_counts_and_worst_times(tmp_path: Path) -> None:
    frames, summary = perf.read_log(write(tmp_path / "a.jsonl", FRAMES))
    line = perf.fixture_line("verbs-zones", frames, summary)
    assert "1200 frames" in line and "3 late" in line
    assert "client worst   3.00 ms p95   3.00 ms" in line
    assert "encode worst   1.50 ms" in line
    assert "drawable wait worst    4.5 ms" in line


def test_a_log_with_no_summary_says_so(tmp_path: Path) -> None:
    frames, summary = perf.read_log(write(tmp_path / "a.jsonl", FRAMES, summary=False))
    assert summary is None
    assert "no summary" in perf.fixture_line("verbs-zones", frames, summary)


def test_late_frames_are_grouped_by_verb_with_the_worst_first() -> None:
    lines = perf.verb_lines(FRAMES)
    assert [line.split()[0] for line in lines] == ["fly", "tap", "(none)"]
    assert "2 late" in lines[0] and "1 late" in lines[1]


def test_lengths_come_from_the_longest_seat_and_round_up_with_a_second() -> None:
    listing = (
        "verbs-zones         --seat you       22152ms, 36 views\n"
        "verbs-zones         --seat opponent  12026ms, 36 views\n"
        "4 recordings. `just b3d info <name>` lists one's moments.\n"
    )
    assert perf.recording_lengths(listing) == {"verbs-zones": 22152}
    assert perf.run_seconds(22152) == 24


def test_a_fixture_is_played_with_every_option_of_just_b3d_app(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    played: list[argparse.Namespace] = []

    def run_app(args: argparse.Namespace) -> int:
        played.append(args)
        return 0

    monkeypatch.setattr(app, "run_app", run_app)
    monkeypatch.setattr(app, "frames_dir", lambda: tmp_path)
    assert perf.play("verbs-zones", 3, "none") is None
    assert played[0].fixture == "verbs-zones" and played[0].run_for == "3s"
    # `run_app` reads every option of `just b3d app`, `--replay` among them.
    assert played[0].no_build and played[0].replay is None
