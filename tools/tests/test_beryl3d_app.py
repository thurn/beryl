"""`just b3d app` and `just b3d frames`: which window a run opens, and which frame log
`frames` reads and how it prints one."""

import json
from pathlib import Path

import pytest

from beryl_tools.beryl3d import app


def test_an_agent_gets_no_window_and_a_person_fullscreen_and_agents_give_for() -> None:
    assert app.choose_window(None, "10s", agent=True) == "none"
    assert app.choose_window(None, None, agent=False) == "fullscreen"
    assert app.choose_window("background", "5s", agent=True) == "background"
    for window in (None, "none", "background"):
        with pytest.raises(ValueError, match="--for"):
            app.choose_window(window, None, agent=True)


def log(folder: Path, started: int, checkout: str, frames: int) -> Path:
    path = folder / f"{started}-{started % 1000}.jsonl"
    run = {
        "checkout": checkout,
        "window": "none",
        "display_hz": 60.0,
        "adapter": "a GPU",
        "load": [1.0, 2.0, 3.0],
        "fixture": "verbs-zones",
        "pause_at_ms": 1500.0,
        "drive": "hover-hand",
        "load_threads": "16:default",
        "slow_gpu_ms": None,
    }
    late = [
        {
            "late": {
                "at_ms": 100.0 * i,
                "since_last_ms": 20.0 + i,
                "waited_ms": 1.0,
                "client_ms": 0.1,
                "encoded_ms": 0.3,
                "gpu_ms": None,
                "verbs": ["fly"],
                "searching": False,
                "held_to_60": False,
            }
        }
        for i in range(12)
    ]
    summary = {
        "summary": {
            "seconds": 3.0,
            "frames": frames,
            "skipped": 0,
            "first_shown_ms": 0.0,
            "late": 12,
            "held_to_60": 0,
            "missed_60": 12,
            "worst_gap_ms": 31.0,
            "worst_wait_ms": 1.0,
            "total_wait_ms": 12.0,
            "median_wait_ms": 0.9,
            "p95_wait_ms": 1.0,
            "after_pause": 0,
            "unbuilt": [],
        }
    }
    lines = [{"run": run}, *late, summary]
    path.write_text("".join(json.dumps(line) + "\n" for line in lines))
    return path


def test_frames_reads_the_newest_log_of_this_checkout(tmp_path: Path) -> None:
    mine = log(tmp_path, 2000, "/here", 10)
    log(tmp_path, 1000, "/here", 5)
    log(tmp_path, 3000, "/elsewhere", 7)
    (tmp_path / "4000-1.jsonl").write_text("not json\n")
    assert app.newest_log(tmp_path, Path("/here")) == mine
    assert app.newest_log(tmp_path, Path("/nowhere")) is None


def test_a_report_gives_the_summary_and_the_ten_worst_frames(tmp_path: Path) -> None:
    printed = app.report(log(tmp_path, 2000, "/here", 10)).splitlines()
    assert any("10 frames in 3.0 s" in line for line in printed)
    assert any("0 frames drawn after the pause" in line for line in printed)
    assert "made to imitate: drive hover-hand, load threads 16:default" in printed
    assert any(
        "median 0.9 ms, 95th percentile 1.0 ms, worst 1.0 ms" in line
        for line in printed
    )
    worst = [line for line in printed if line.startswith("  at ")]
    assert len(worst) == app.WORST
    assert "gap 31.0" in worst[0] and "gap 22.0" in worst[-1]
