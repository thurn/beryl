"""`just b3d motion-film`: the films it asks `b3d-render` for and what it prints, with the
programs stubbed."""

import argparse
import json
from pathlib import Path
from typing import Any

import pytest

from beryl_tools.beryl3d import motion_film


def step(view: int, verb: str, case: str | None, start: float, end: float) -> str:
    return json.dumps(
        {
            "recording": "verbs-zones",
            "seat": "you",
            "view": view,
            "beat": 0,
            "step": 0,
            "verb": verb,
            "case": case,
            "start": start,
            "end": end,
            "effects": [],
            "sounds": [],
        }
    )


TIMELINE: str = "\n".join(
    [
        "just b3d timeline verbs-zones --seat you --size 1600x900",
        step(2, "set", "step begins", 0, 58),
        step(4, "fly", "own spell cast", 0, 370),
        step(5, "fly", "own spell cast", 0, 400),
    ]
)
INFO: str = "\n".join(
    [
        "just b3d info verbs-zones --seat you --size 1600x900",
        "verbs-zones: 36 views, 11 prompts, 106 steps, ends at 22152ms",
        "step:2.0.0       29ms  set from 0ms to 58ms",
        "step:4.0.0      643ms  fly card #6 from 458ms to 828ms",
    ]
)


def run(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> tuple[list[list[str]], str]:
    """Runs `just b3d motion-film fly` with no Arena recording on the machine, and returns
    each command line given to a program and what the command printed."""
    calls: list[list[str]] = []

    def run_built(name: str, arguments: list[str]) -> str:
        calls.append([name, *arguments])
        if arguments[0] == "timeline":
            return TIMELINE
        if arguments[0] == "info":
            return INFO
        return f"wrote {tmp_path}/film-{len(calls)}.png (760x427 px)\n  #1 458ms  nothing moving\n"

    def no_arena(*_args: Any) -> tuple[list[dict[str, Any]], list[str]]:
        return [], []

    monkeypatch.setattr(motion_film, "run_built", run_built)
    monkeypatch.setattr(motion_film, "build", lambda: 0)
    monkeypatch.setattr(motion_film, "arena_side", no_arena)
    monkeypatch.setattr(motion_film.paths, "cache", lambda: tmp_path)
    args = argparse.Namespace(verb="fly", case=[], recording=None, instances=2)
    assert motion_film.run_motion_film(args) == 0
    return calls, capsys.readouterr().out


def test_each_film_is_taken_at_arenas_rate_and_width_from_both_seats_and_layouts(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    calls, _ = run(monkeypatch, tmp_path, capsys)
    films = [c for c in calls if c[0] == "b3d-render"]
    seen = {(c[c.index("--seat") + 1], c[c.index("--layout") + 1]) for c in films}
    assert seen == {(s, l) for s in ("you", "opponent") for l in ("desktop", "phone")}
    for film in films:
        line = " ".join(film)
        assert "--rate 15 --frame-width 760" in line, line
        # The first fly of the view at 458 ms, until 400 ms of settling past its end.
        assert "--from 458ms --to 1228ms" in line, line


def test_with_no_arena_recording_only_beryl_3ds_side_is_filmed_and_said_on_one_line(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, printed = run(monkeypatch, tmp_path, capsys)
    notices = [line for line in printed.splitlines() if line.startswith("Arena:")]
    assert len(notices) == 1 and "only Beryl 3D's side" in notices[0], printed
    assert "Beryl 3D desktop/you:" in printed
    assert "desktop / you / own spell cast" in printed


def test_a_step_longer_than_one_film_is_filmed_in_several() -> None:
    # 24 frames at 15 a second are 1600 ms.
    assert motion_film.spans(0, 2000) == [(0, 1600), (1600, 2000)]
    assert motion_film.spans(100, 500) == [(100, 500)]
