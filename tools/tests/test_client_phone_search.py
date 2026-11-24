"""`just client phone-search`: the numbers it reads from a phone's frame log, the rule that calls
the frame thread starved, and how it waits for the summary a suspended app writes."""

import argparse
import json
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from beryl_tools.client import phone_search
from beryl_tools.client.phone_log import lines

RUN: dict[str, Any] = {
    "run": {"adapter": "Apple Paravirtual (Metal)", "load": [21.5, 20.0, 19.0]}
}


def late(since_last_ms: float, searching: bool) -> dict[str, Any]:
    return {"late": {"since_last_ms": since_last_ms, "searching": searching}}


def summary(
    frames: int, late: int, searching_frames: int, searching_late: int
) -> dict[str, Any]:
    return {
        "summary": {
            "seconds": 300.0,
            "frames": frames,
            "late": late,
            "worst_gap_ms": 90.0,
            "searching": {"frames": searching_frames, "late": searching_late},
        }
    }


def log(*parsed: dict[str, Any]) -> str:
    return "".join(json.dumps(line) + "\n" for line in parsed)


@pytest.mark.parametrize("gles", [False, True])
def test_windows_phone_search_runs_only_android(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, gles: bool
) -> None:
    monkeypatch.setattr(phone_search, "sys", SimpleNamespace(platform="win32"))
    monkeypatch.setattr(phone_search.system, "loadavg", lambda: (0.0, 0.0, 0.0))
    monkeypatch.setattr(phone_search.phone, "out_dir", lambda _kind: tmp_path)
    calls: list[bool] = []

    def run_android(use_gles: bool) -> tuple[str, None]:
        calls.append(use_gles)
        return log(RUN, summary(100, 1, 50, 0)), None

    def run_ios(_run: Path) -> None:
        pytest.fail("phone-search attempted to start the iOS Simulator on Windows")

    monkeypatch.setattr(phone_search, "run_android", run_android)
    monkeypatch.setattr(phone_search, "run_ios", run_ios)

    assert phone_search.run_phone_search(argparse.Namespace(gles=gles)) == 0
    assert calls == ([False, True] if gles else [False])


def test_the_numbers_split_frames_and_late_frames_by_search() -> None:
    text = log(
        RUN,
        late(40.0, True),
        late(90.0, False),
        late(55.0, True),
        summary(frames=1000, late=30, searching_frames=400, searching_late=10),
    )
    found = phone_search.numbers(lines(text))
    assert found is not None
    assert (found.searching_frames, found.searching_late) == (400, 10)
    assert (found.idle_frames, found.idle_late) == (600, 20)
    assert found.worst_gap_searching_ms == 55.0
    assert found.load == 21.5
    assert not found.starved


def test_late_frames_over_twice_the_share_while_searching_is_starvation() -> None:
    # 20% late while searching against 8% while not.
    starved = phone_search.numbers(
        lines(
            log(
                RUN,
                summary(frames=600, late=56, searching_frames=100, searching_late=20),
            )
        )
    )
    assert starved is not None and starved.starved
    # Nothing drawn while not searching leaves nothing to compare.
    alone = phone_search.numbers(
        lines(
            log(
                RUN, summary(frames=10, late=10, searching_frames=10, searching_late=10)
            )
        )
    )
    assert alone is not None and not alone.starved


def test_a_log_with_no_summary_has_no_numbers() -> None:
    assert phone_search.numbers(lines(log(RUN, late(40.0, True)))) is None


def test_the_run_suspends_after_its_time_and_waits_for_the_new_summary() -> None:
    now: list[float] = [0.0]
    written: list[str] = [log(RUN)]
    suspended: list[bool] = []

    def suspend() -> None:
        suspended.append(True)
        written.append(log(RUN, summary(10, 0, 5, 0)))

    def pause(seconds: float) -> None:
        now[0] += seconds

    text, fault = phone_search.play(
        lambda: written[-1],
        lambda: True,
        lambda: "",
        suspend,
        "ios",
        play_s=20,
        pause=pause,
        clock=lambda: now[0],
    )
    assert fault is None and suspended == [True]
    assert text is not None and phone_search.summary_count(text) == 1


def test_a_game_that_ends_stops_the_wait_early_and_the_app_is_still_suspended() -> None:
    over: str = log(RUN, summary(10, 0, 5, 0))
    written: list[str] = [over]

    def suspend() -> None:
        written.append(over + log(summary(12, 0, 5, 0)))

    text, fault = phone_search.play(
        lambda: written[-1],
        lambda: True,
        lambda: "",
        suspend,
        "ios",
        pause=lambda s: None,
    )
    assert fault is None
    assert text is not None and phone_search.summary_count(text) == 2


def test_an_app_that_exits_is_a_fault() -> None:
    _, fault = phone_search.play(
        lambda: log(RUN), lambda: False, lambda: "", lambda: None, "android"
    )
    assert fault == "the app exited and wrote no error"
