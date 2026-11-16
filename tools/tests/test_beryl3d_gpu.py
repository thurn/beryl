"""`just b3d gate --gpu`: holding fragment counts and shader cost to their limits."""

import json

from beryl_tools.beryl3d import gpu


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


def test_a_board_over_its_limit_names_the_frame() -> None:
    boards = gpu.parse_boards(output([1000, 1800]))
    for loose in (False, True):
        # 1.8 screens is over the target, so it fails during the build-out too.
        [line], _ = gpu.fragment_failures(
            boards, {"stress-255": 1.75}, warn_within_target=loose
        )
        assert "stress-255 frame 31" in line
        assert "1.8 screens (1800), over its limit 1.75" in line
    assert gpu.fragment_failures(
        boards, {"stress-255": 1.8}, warn_within_target=False
    ) == ([], [])


def test_during_the_build_out_a_board_within_the_target_only_warns() -> None:
    boards = gpu.parse_boards(output([1000, 1100]))
    failed, warned = gpu.fragment_failures(
        boards, {"stress-255": 1.05}, warn_within_target=True
    )
    assert failed == []
    [line] = warned
    assert "1.1 screens (1100), over its limit 1.05, within the target 1.75" in line
    failed, _ = gpu.fragment_failures(
        boards, {"stress-255": 1.05}, warn_within_target=False
    )
    assert len(failed) == 1


def test_a_board_with_no_limit_fails() -> None:
    boards = gpu.parse_boards(output([1000]))
    [line], _ = gpu.fragment_failures(boards, {}, warn_within_target=True)
    assert "no limit" in line


def test_update_lowers_and_never_raises() -> None:
    boards = gpu.parse_boards(output([1200]))
    assert gpu.lowered_fragments(boards, {}) == {"stress-255": 1.2}
    assert gpu.lowered_fragments(boards, {"stress-255": 1.5}) == {"stress-255": 1.2}
    assert gpu.lowered_fragments(boards, {"stress-255": 1.0}) == {"stress-255": 1.0}


def test_material_counts_are_read_from_the_test_output() -> None:
    text = (
        "running 2 tests\n"
        '.materials {"card":{"expressions":63,"samples":1}}\n'
        "test result: ok.\n"
    )
    measured = gpu.parse_materials(text)
    assert measured == {"card": {"expressions": 63, "samples": 1}}
    lowered = gpu.lowered_materials(
        measured, {"card": {"expressions": 60, "samples": 2}}
    )
    assert lowered == {"card": {"expressions": 60, "samples": 1}}
