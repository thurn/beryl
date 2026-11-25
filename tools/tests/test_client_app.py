"""`just client app` and `just client frames`: which window a run opens, and which frame log
`frames` reads and how it prints one."""

import json
from pathlib import Path

import pytest

from beryl_tools.client import app, frame_trace


def test_an_agent_gets_no_window_and_a_person_fullscreen_and_agents_give_for() -> None:
    assert app.choose_window(None, "10s", agent=True) == "none"
    assert app.choose_window(None, None, agent=False) == "fullscreen"
    assert app.choose_window("background", "5s", agent=True) == "background"
    for window in (None, "none", "background"):
        with pytest.raises(ValueError, match="--for"):
            app.choose_window(window, None, agent=True)


def log(
    folder: Path, started: int, checkout: str, frames: int, missing_glyphs: int = 0
) -> Path:
    path = folder / f"{started}-{started % 1000}.jsonl"
    run = {
        "checkout": checkout,
        "window": "none",
        "display_hz": 120.0,
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
                "hz": 120.0 if i < 6 else 60.0,
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
            "hz": 60.0,
            "steps": [{"at_ms": 550.0, "hz": 60.0}],
            "missing_glyphs": missing_glyphs,
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
    assert "asked for 120.0 Hz, then 60.0 Hz at 550 ms; 60.0 Hz at the end" in printed
    worst = [line for line in printed if line.startswith("  at ")]
    assert len(worst) == app.WORST
    assert "gap 31.0 at 60.0 Hz" in worst[0] and "gap 22.0 at 120.0 Hz" in worst[-1]


def test_a_run_that_ends_with_glyphs_missing_fails(tmp_path: Path) -> None:
    assert app.undelivered(log(tmp_path, 1000, "/here", 10)) is None
    failure = app.undelivered(log(tmp_path, 2000, "/here", 10, missing_glyphs=3))
    assert failure is not None and "3 glyphs" in failure


def test_live_timing_windows_explain_gaps_and_exclude_pauses(tmp_path: Path) -> None:
    attached = tmp_path / "report"
    attached.mkdir()
    path = log(attached, 2000, "/here", 10)
    target = path.rename(attached / "frames.jsonl")
    run = target.read_text(encoding="utf-8").splitlines()[0]

    def frame(
        sequence: int, epoch: int, callback_ms: int, delay_ms: int, outcome: str
    ) -> dict[str, object]:
        callback = callback_ms * 1_000_000
        handled = callback + delay_ms * 1_000_000
        present = 42_000_000 if sequence == 2 else 1_000_000
        return {
            "frame": {
                "sequence": sequence,
                "epoch": epoch,
                "requested_hz": 60.0,
                "callback_ns": callback,
                "display_ns": callback - 4_000_000,
                "target_ns": callback + 12_000_000,
                "reported_hz": 60.0,
                "handled_ns": handled,
                "finished_ns": handled + present + 1_000_000,
                "outcome": outcome,
                "submitted_ns": handled + 1_000_000 if outcome == "submitted" else None,
                "cpu": {"present_call_ns": present, "prepare_ns": 3_000_000},
                "render_size": [3200, 1800],
                "fullscreen": False,
                "tier": "enhanced",
                "animating": True,
                "searching": True,
                "gpu_in_flight": 1,
                "presented_ns": None,
            }
        }

    change = {
        "pacing_change": {
            "at_ns": 90_000_000,
            "previous_hz": 120.0,
            "new_hz": 60.0,
            "previous_tier": "enhanced",
            "new_tier": "enhanced",
            "adaptive": {
                "gap_ns": 40_000_000,
                "work_ns": 28_000_000,
                "missed": 3,
                "window": 60,
                "misses_needed": 3,
                "roomy": 0,
                "roomy_needed": 600,
            },
        }
    }
    window = {
        "timing_window": {
            "reason": "bug_saved",
            "end_ns": 1_050_000_000,
            "events": [
                frame(1, 1, 100, 0, "submitted"),
                frame(2, 1, 116, 0, "submitted"),
                frame(3, 1, 170, 18, "timeout"),
                frame(4, 1, 204, 0, "submitted"),
                {"pacer": {"running": False, "at_ns": 210_000_000, "epoch": 1}},
                frame(5, 1, 215, 0, "idle"),
                {"pacer": {"running": True, "at_ns": 1_000_000_000, "epoch": 2}},
                frame(6, 2, 1000, 0, "submitted"),
                frame(7, 2, 1016, 0, "submitted"),
                {
                    "gpu": {
                        "sequence": 2,
                        "submit_ns": 117_000_000,
                        "observed_ns": 123_000_000,
                        "main_pass_gpu_ns": 2_000_000,
                    }
                },
            ],
        }
    }
    target.write_text(
        run + "\n" + json.dumps(change) + "\n" + json.dumps(window) + "\n{",
        encoding="utf-8",
    )
    summary = app.report(attached)
    assert "callbacks 33.3 Hz, submissions 25.0 Hz" in summary
    assert "90.00 ms: 120 → 60 Hz" in summary
    assert "skipped idle 1, timeout 1, 1 pauses and 1 resumes" in summary
    assert "#2 → #3: 54.00 ms" in summary
    assert "queued 18.00 ms, previous handler 43.00 ms" in summary
    assert "CPU presentation call and hook: 1.00 / 42.00 / 42.00 / 42.00" in summary
    assert "main render pass GPU time: 2.00 / 2.00 / 2.00 / 2.00" in summary
    assert "actual presentation timestamps: 0 available" in summary


def timing(callback_ms: float, **changes: object) -> dict[str, object]:
    """A tick's timing record at 120 Hz, handled at once in 1 ms with no wait."""
    callback = int(callback_ms * 1_000_000)
    row: dict[str, object] = {
        "sequence": 1,
        "epoch": 1,
        "requested_hz": 120.0,
        "callback_ns": callback,
        "display_ns": callback - 1_000_000,
        "handled_ns": callback,
        "finished_ns": callback + 1_000_000,
        "outcome": "submitted",
        "cpu": {"client_ns": 300_000, "drawable_wait_ns": 100_000},
    }
    row.update(changes)
    return row


def late_line(frame: object, previous: object) -> dict[str, object]:
    line: dict[str, object] = {"waited_ms": 0.1, "hz": 120.0}
    if frame is not None:
        line["frame"] = frame
        line["previous"] = previous
    return line


def test_each_late_frame_is_given_its_cause() -> None:
    previous = timing(0)
    # Each record below is the late frame, 25 ms after the one before unless the display
    # timestamps put it nearer, changed to show one cause.
    cases = {
        "drawable": timing(25, cpu={"drawable_wait_ns": 6_000_000}),
        "handler": (timing(25), timing(0, finished_ns=10_000_000)),
        "queued": timing(25, handled_ns=30_000_000),
        "callback late": timing(25, display_ns=19_000_000),
        "link skipped": timing(25),
        "handling only": timing(10),
        "unknown": None,
    }
    assert tuple(cases) == frame_trace.CAUSES
    for expected, case in cases.items():
        frame, before = case if isinstance(case, tuple) else (case, previous)
        found = frame_trace.cause(
            late_line(frame, before), median_offset_ns=1_000_000, refresh_ns=8.3e6
        )
        assert found == expected, (expected, found)


def test_a_report_uses_the_last_summary_and_counts_causes_by_rate(
    tmp_path: Path,
) -> None:
    path = log(tmp_path, 2000, "/here", 10)
    later = {
        "summary": {
            **json.loads(path.read_text().splitlines()[-1])["summary"],
            "frames": 99,
            "seconds": 33.0,
            "rates": [
                {"hz": 120.0, "seconds": 20.0, "frames": 2400, "late": 6},
                {"hz": 60.0, "seconds": 13.0, "frames": 780, "late": 6},
            ],
        }
    }
    with path.open("a") as stream:
        stream.write(json.dumps(later) + "\n")
    printed = app.report(path).splitlines()
    assert any("99 frames in 33.0 s" in line for line in printed)
    assert not any("10 frames in 3.0 s" in line for line in printed)
    assert "  120 Hz: 20.0 s, 2400 frames, 6 late (0.25%)" in printed
    table = printed.index("late frames by cause and rate asked for:")
    assert printed[table + 1].split() == ["cause", "120", "Hz", "60", "Hz", "all"]
    # The fixture's late lines predate timing records, so every one is `unknown`.
    assert printed[table + 1 + len(frame_trace.CAUSES)].split() == [
        "unknown",
        "6",
        "6",
        "12",
    ]
