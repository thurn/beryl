"""`just b3d gate --counts`: reading Gungraun's counts of a board's frames and holding them to
their limits."""

import json

from beryl_tools.beryl3d import counts, gate

BOARD = gate.Board(
    name="stress-255",
    frames=[],
    handle_allocations=0,
    first_allocation=None,
    ranked=(30, 31),
    stretch=(29, 33),
)


def summary(function: str, id: str, instructions: int) -> str:
    """One of Gungraun's JSON lines."""
    return json.dumps(
        {
            "module_path": f"frame_counts::frames::{function}",
            "id": id,
            "profiles": [
                {
                    "tool": "Callgrind",
                    "data": {
                        "total": {"metrics": {"Ir": {"values": {"new": instructions}}}}
                    },
                }
            ],
        }
    )


def output(ranked: list[int], stretch: list[int], total: int = 1000) -> str:
    lines = ["Compiling beryl_inspect", summary("total", "stress_255", total)]
    lines += [summary("ranked", f"stress_255_{i}", n) for i, n in enumerate(ranked)]
    lines += [summary("stretch", f"stress_255_{i}", n) for i, n in enumerate(stretch)]
    return "\n".join(lines)


def measured(
    ranked: list[int], stretch: list[int], total: int = 1000
) -> counts.Counted:
    found = counts.counted(BOARD, counts.summaries(output(ranked, stretch, total)))
    assert found is not None
    return found


def test_each_run_counts_the_frame_in_its_place() -> None:
    found = measured([50, 70], [10, 50, 70, 20])
    assert found.total == 1000
    assert found.ranked == {30: 50, 31: 70}
    assert found.stretch == {29: 10, 30: 50, 31: 70, 32: 20}
    assert counts.worst(found) == (31, 70)


def test_a_missing_run_is_no_count() -> None:
    found = counts.summaries(output([50], [10, 50, 70, 20]))
    assert counts.counted(BOARD, found) is None


def test_the_worst_frame_fails_past_its_tolerance_while_the_total_passes() -> None:
    limits = {
        "worst_frame_instructions": 69,
        "total_instructions": 1000,
        "tolerance_percent": 1.0,
    }
    failed, _ = counts.judge("stress-255", measured([50, 70], [10, 50, 70, 20]), limits)
    assert len(failed) == 1
    assert "frame 31: worst_frame_instructions 70" in failed[0]
    within = {**limits, "worst_frame_instructions": 70}
    failed, said = counts.judge(
        "stress-255", measured([50, 70], [10, 50, 70, 20], 1009), within
    )
    assert failed == []
    assert len(said) == 2


def test_a_frame_the_counters_did_not_rank_fails() -> None:
    limits = {"worst_frame_instructions": 70, "total_instructions": 1000}
    failed, _ = counts.judge("stress-255", measured([50, 70], [10, 50, 70, 90]), limits)
    assert len(failed) == 1
    assert "frame 32 has the most instructions of frames 29 to 32" in failed[0]


def test_update_sets_missing_limits_and_never_raises_one() -> None:
    found = measured([50, 70], [10, 50, 70, 20])
    limits = counts.lowered("stress-255", found, {})
    assert limits == {
        "worst_frame_instructions": 70,
        "total_instructions": 1000,
        "tolerance_percent": 1.0,
    }
    old = {"worst_frame_instructions": 60, "total_instructions": 2000}
    limits = counts.lowered("stress-255", found, old)
    assert limits["worst_frame_instructions"] == 60
    assert limits["total_instructions"] == 1000
    assert (
        counts.judge("stress-255", found, {**limits, "worst_frame_instructions": 70})[0]
        == []
    )


def test_the_gate_binary_names_the_frames_to_count() -> None:
    text = json.dumps(
        {
            "boards": [
                {
                    "name": "stress-255",
                    "handle_allocations": 0,
                    "frames": [],
                    "first_frame_allocation": None,
                    "ranked": [30, 31],
                    "stretch": [29, 33],
                }
            ]
        }
    )
    (board,) = gate.parse(text)
    assert board.ranked == (30, 31)
    assert board.stretch == (29, 33)
