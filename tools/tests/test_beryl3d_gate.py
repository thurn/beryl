"""`just b3d gate`: holding a board's counts to its limits and lowering them."""

import json

from beryl_tools.beryl3d import gate


def output(records: list[int], allocation: int | None = None) -> str:
    """The `gate` binary's output for one board whose frames write `records`."""
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
            "draw_calls": 1,
            "instances": n,
            "upload_bytes": 80 * n,
            "blended_area": 0.123401,
            "frame_allocations": 0,
        }
        for i, n in enumerate(records)
    ]
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
    return json.dumps({"boards": [board]})


def test_update_sets_missing_limits_from_the_measurement() -> None:
    boards = gate.parse(output([3, 5]))
    limits = gate.lowered(boards, {})["stress-255"]
    assert limits["worst_frame_records"] == 5
    assert limits["total_records"] == 8
    assert limits["worst_frame_upload_bytes"] == 400
    # Blended area is rounded up, so the measurement stays within its own limit.
    assert limits["worst_frame_blended_area"] == 0.1235
    assert limits["handle_allocations"] == 7
    assert limits["frame_allocations"] == 0
    assert gate.failures(boards, {"stress-255": limits}) == []


def test_update_lowers_limits_and_never_raises_one() -> None:
    boards = gate.parse(output([3, 5]))
    old = {"stress-255": {"worst_frame_records": 4, "total_records": 100}}
    limits = gate.lowered(boards, old)["stress-255"]
    assert limits["worst_frame_records"] == 4
    assert limits["total_records"] == 8


def test_a_count_over_its_limit_names_the_board_frame_and_count() -> None:
    boards = gate.parse(output([3, 5]))
    limits = gate.lowered(boards, {})
    limits["stress-255"]["worst_frame_records"] = 4
    assert gate.failures(boards, limits) == [
        "stress-255 frame 1 (16 ms, flight): worst_frame_records is 5, over its limit 4"
    ]


def test_an_allocation_in_a_frame_fails_with_its_backtrace() -> None:
    boards = gate.parse(output([3, 5], allocation=1))
    lines = gate.failures(boards, gate.lowered(boards, {}))
    assert lines[0].startswith("stress-255 frame 1 (16 ms, flight): frame_allocations")
    assert "at Client::frame" in lines[-1]


def test_a_count_with_no_limit_fails() -> None:
    boards = gate.parse(output([3]))
    lines = gate.failures(boards, {})
    assert any("worst_frame_records is 3 with no limit" in line for line in lines)


def test_a_count_under_its_limit_fails_until_the_limit_is_lowered() -> None:
    # A count left under its limit is room a later change could fill without anyone seeing.
    boards = gate.parse(output([3, 5]))
    limits = gate.lowered(boards, {})
    limits["stress-255"]["total_records"] = 9
    assert gate.failures(boards, limits) == [
        "stress-255 in total: total_records is 8, under its limit 9;"
        " `just b3d gate --update` lowers it"
    ]
    assert gate.failures(boards, gate.lowered(boards, limits)) == []
