"""`just client gate --gpu`: holding fragment counts and shader cost to their limits."""

import json

from beryl_tools.client import gate, gpu


def output(counts: list[int], pixels: int = 1000) -> str:
    """The `fragments` example's output for one board whose ranked frames shade `counts`."""
    frames = [
        {"frame": 30 + i, "ms": 500 + i * 16, "phase": "flight", "fragments": n}
        for i, n in enumerate(counts)
    ]
    board = {"name": "stress-255", "pixels": pixels, "frames": frames}
    return json.dumps({"adapter": "llvmpipe", "boards": [board], "recorded": []})


def test_the_worst_frame_is_held_in_screens_rounded_up() -> None:
    [board] = gpu.parse_boards(output([1500, 1700, 1700], pixels=3))
    assert board.worst().frame == 31
    # 1700 / 3 is 566.666..., kept to six places and rounded up.
    assert board.screens() == 566.666667


def fragment_budgets(tier: str, maximum: float) -> dict[str, dict[str, gate.Budget]]:
    return {
        "stress-255": {
            f"{tier}_fragment_screens": {"max": maximum, "why": "test budget"}
        }
    }


def test_a_board_over_its_budget_names_the_frame() -> None:
    boards = gpu.parse_boards(output([1000, 1800]))
    [line] = gpu.fragment_failures(
        boards, fragment_budgets("baseline", 1.75), "baseline"
    )
    assert "stress-255 frame 31" in line
    assert "1.8 screens (1800), over its budget 1.75" in line


def test_a_board_under_its_budget_passes() -> None:
    boards = gpu.parse_boards(output([1000, 1100]))
    assert (
        gpu.fragment_failures(boards, fragment_budgets("baseline", 1.75), "baseline")
        == []
    )


def test_a_board_with_no_budget_fails() -> None:
    boards = gpu.parse_boards(output([1000]))
    [line] = gpu.fragment_failures(boards, {}, "baseline")
    assert "baseline_fragment_screens is 1.0 screens (1000) with no budget" in line


def test_material_counts_are_read_from_the_test_output() -> None:
    text = (
        "running 2 tests\n"
        '.materials {"card":{"expressions":63,"samples":1}}\n'
        "test result: ok.\n"
    )
    measured = gpu.parse_materials(text)
    assert measured == {"card": {"expressions": 63, "samples": 1}}


def tiered(counts: list[int], tier: str, draw_calls: int = 20) -> str:
    """The `fragments` example's output in `tier` for one board whose ranked frames shade
    `counts` of 1000 pixels, each taking `draw_calls` draw calls."""
    text = json.loads(output(counts))
    text["tier"] = tier
    for frame in text["boards"][0]["frames"]:
        frame["draw_calls"] = draw_calls
    return json.dumps(text)


def test_an_enhanced_board_uses_its_own_fragment_budget() -> None:
    boards = gpu.parse_boards(tiered([1000, 4100], "enhanced"))
    [line] = gpu.fragment_failures(
        boards, fragment_budgets("enhanced", 4.0), "enhanced"
    )
    assert line.startswith("stress-255 frame 31 (516 ms, flight) in enhanced:")
    assert "4.1 screens (4100), over its budget 4.0" in line

    boards = gpu.parse_boards(tiered([1000, 3900], "enhanced"))
    budgets = fragment_budgets("baseline", 1.75)
    budgets["stress-255"].update(fragment_budgets("enhanced", 4.0)["stress-255"])
    assert gpu.fragment_failures(boards, budgets, "enhanced") == []


def test_an_enhanced_frame_over_32_draw_calls_fails() -> None:
    assert (
        gpu.draw_call_failures(gpu.parse_boards(tiered([1000], "enhanced", 32)), 32)
        == []
    )
    [line] = gpu.draw_call_failures(
        gpu.parse_boards(tiered([1000], "enhanced", 33)), 32
    )
    assert line == (
        "stress-255 frame 30 (500 ms, flight) in enhanced: 33 draw calls, over the budget 32"
    )


def test_each_tier_is_read_from_its_two_runs() -> None:
    runs = [tiered([1000], "baseline"), tiered([2000], "enhanced")] * 2
    counted = gpu.compare_runs(runs)
    assert [b.worst().fragments for b in counted["baseline"]] == [1000]
    assert [b.worst().fragments for b in counted["enhanced"]] == [2000]
