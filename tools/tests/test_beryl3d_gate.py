"""`just b3d gate`: holding a board's counts to its budgets."""

import argparse
import json

import pytest

from beryl_tools.beryl3d import gate


def output(
    records: list[int],
    allocation: int | None = None,
    particles: list[int] | None = None,
    resolved: list[int] | None = None,
) -> str:
    """The `gate` binary's output for one board whose frames write `records`, and whose
    Enhanced playback's frames have `particles` and `resolved` elements (none and 10 each by
    default)."""
    frames = [
        {
            "frame": i,
            "ms": i * 16,
            "phase": "rest" if i == 0 else "flight",
            "elements": 10,
            "segments": 0,
            "records": n,
            "layout_nodes": 0,
            "text_runs": 0,
            "placed": 0,
            "steps": 0,
            "draw_calls": 1,
            "instances": n,
            "upload_bytes": 80 * n,
            "blended_area": 0.123401,
            "frame_allocations": 0,
            "resolved": 10,
        }
        for i, n in enumerate(records)
    ]
    enhanced = {
        "name": "stress-255 enhanced",
        "frames": [
            {
                "frame": i,
                "ms": i * 16,
                "phase": "rest" if i == 0 else "flight",
                "particles": (particles or [0] * len(records))[i],
                "resolved": (resolved or [10] * len(records))[i],
            }
            for i in range(len(records))
        ],
    }
    first = None
    if allocation is not None:
        frames[allocation]["frame_allocations"] = 2
        first = {"frame": allocation, "backtrace": "at Client::frame"}
    board = {
        "name": "stress-255",
        "handle_allocations": 7,
        "frames": frames,
        "first_frame_allocation": first,
    }
    return json.dumps({"boards": [board], "enhanced": [enhanced]})


def board_budgets(board: gate.Board) -> dict[str, gate.Budget]:
    return {
        m.key: gate.Budget(max=gate.number(m.key, m.value), why="test budget")
        for m in gate.measure(board)
        if m.key != "frame_allocations"
    }


def test_a_count_over_its_budget_names_the_board_frame_and_count() -> None:
    [board] = gate.parse(output([3, 5]))
    budgets = board_budgets(board)
    budgets["worst_frame_records"]["max"] = 4
    assert gate.failures([board], {board.name: budgets}) == [
        "stress-255 frame 1 (16 ms, flight): worst_frame_records is 5, over its budget 4"
    ]


def test_an_allocation_in_a_frame_fails_with_its_backtrace() -> None:
    [board] = gate.parse(output([3, 5], allocation=1))
    budgets = board_budgets(board)
    budgets["frame_allocations"] = {"max": 10, "why": "Ignored by the hard zero check"}
    lines = gate.failures([board], {board.name: budgets})
    assert lines[0] == (
        "stress-255 frame 1 (16 ms, flight): frame_allocations is 2, must be zero"
    )
    assert "at Client::frame" in lines[-1]


def test_a_count_without_a_budget_fails() -> None:
    boards = gate.parse(output([3]))
    lines = gate.failures(boards, {})
    assert any("worst_frame_records is 3 with no budget" in line for line in lines)


def test_a_count_under_its_budget_passes() -> None:
    [board] = gate.parse(output([3, 5]))
    budgets = board_budgets(board)
    budgets["total_records"]["max"] = 9
    assert gate.failures([board], {board.name: budgets}) == []


def test_json_reports_the_measurements_even_when_a_budget_fails(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    text = output([3, 5], particles=[0, 6])
    [board] = gate.parse(text)
    enhanced = gate.parse_enhanced(text)
    budgets: gate.Budgets = {
        "boards": {board.name: board_budgets(board)},
        "enhanced": {"particles": {"max": 5, "why": "test budget"}},
        "scaling": {},
        "trend": {"adjacent_percent": 5, "weekly_percent": 10},
    }
    monkeypatch.setattr(gate, "read_budgets", lambda: budgets)
    monkeypatch.setattr(
        gate, "play", lambda _names, *, json_output: ([board], enhanced)
    )
    parser = argparse.ArgumentParser()
    gate.gate_arguments(parser)
    assert gate.run_gate(parser.parse_args(["--json"])) == 1
    captured = capsys.readouterr()
    measured = json.loads(captured.out)["boards"][board.name]
    assert all(measured[m.key] == m.value for m in gate.measure(board))
    assert measured["worst_frame_blended_area"] == 0.123401
    assert measured["enhanced_worst_frame_particles"] == 6
    assert "particles is 6, over its budget 5" in captured.err


def enhanced_failures(text: str, particles: int | None = 2000) -> list[str]:
    return gate.enhanced_failures(
        gate.parse(text), gate.parse_enhanced(text), particles
    )


def test_the_enhanced_tier_within_its_limits_passes() -> None:
    assert enhanced_failures(output([3, 5], particles=[0, 2000])) == []


def test_an_enhanced_frame_over_the_particle_limit_fails() -> None:
    assert enhanced_failures(output([3, 5, 4], particles=[0, 2001, 1500])) == [
        "stress-255 enhanced frame 1 (16 ms, flight): particles is 2001, over its"
        " budget 2000"
    ]


def test_an_enhanced_frame_with_other_resolved_elements_fails() -> None:
    [line] = enhanced_failures(output([3, 5], resolved=[10, 11]))
    assert line == (
        "stress-255 enhanced frame 1 (16 ms, flight): 11 resolved elements, but"
        " Baseline has 10"
    )


def test_no_particle_limit_fails() -> None:
    [line] = enhanced_failures(output([3]), particles=None)
    assert "no budget under enhanced.particles" in line
