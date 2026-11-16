"""`just b3d gate --counts`: reading Callgrind's per-frame counts of a board and holding them to
their limits."""

from beryl_tools.beryl3d import counts, vm

RANKED = (1, 2)


def output(board: str, frames: list[int]) -> str:
    """The machine script's lines for one board whose frames counted `frames`, among the build's
    own output."""
    lines = ["   Compiling beryl_inspect", f"frames {board} {len(frames)}"]
    lines += [f"count {board} {dump} {n}" for dump, n in enumerate(frames, start=1)]
    return "\n".join(lines)


def measured(frames: list[int]) -> counts.Counted:
    found = counts.parse(output("stress-255", frames))
    return found["stress-255"]


def test_dump_n_counts_frame_n_minus_one() -> None:
    found = measured([10, 50, 70, 20])
    assert found.frames == {0: 10, 1: 50, 2: 70, 3: 20}
    assert found.total == 150
    assert counts.worst(found) == (2, 70)


def test_boards_counted_at_once_keep_their_own_dumps() -> None:
    text = output("stress-255", [1, 2]) + "\n" + output("pay-default", [3])
    found = counts.parse(text)
    assert found["stress-255"].frames == {0: 1, 1: 2}
    assert found["pay-default"].frames == {0: 3}


def test_a_board_missing_a_dump_is_no_count() -> None:
    text = "frames stress-255 3\ncount stress-255 1 10\ncount stress-255 3 30"
    assert counts.parse(text) == {}


def test_the_worst_frame_fails_past_its_tolerance_while_the_total_passes() -> None:
    limits = {
        "worst_frame_instructions": 69,
        "total_instructions": 150,
    }
    failed, _ = counts.judge(
        "stress-255",
        measured([10, 50, 70, 20]),
        limits,
        RANKED,
        tolerance=1.0,
        fail_below=True,
    )
    assert len(failed) == 1
    assert "frame 2: worst_frame_instructions 70" in failed[0]
    within = {**limits, "worst_frame_instructions": 70, "total_instructions": 1000}
    failed, said = counts.judge(
        "stress-255",
        measured([10, 50, 70, 20, 849]),
        {**within, "worst_frame_instructions": 849},
        (4,),
        tolerance=1.0,
        fail_below=True,
    )
    assert failed == []
    assert len(said) == 2


def test_a_frame_the_counters_did_not_rank_fails() -> None:
    limits = {"worst_frame_instructions": 90, "total_instructions": 220}
    failed, _ = counts.judge(
        "stress-255",
        measured([10, 50, 70, 90]),
        limits,
        RANKED,
        tolerance=1.0,
        fail_below=True,
    )
    assert len(failed) == 1
    assert "frame 3 has the most instructions (90)" in failed[0]


def test_update_sets_missing_limits_and_never_raises_one() -> None:
    found = measured([10, 50, 70, 20])
    limits = counts.lowered("stress-255", found, {})
    assert limits == {"worst_frame_instructions": 70, "total_instructions": 150}
    # A board's own tolerance from before build_out.py is dropped.
    old = {
        "worst_frame_instructions": 60,
        "total_instructions": 2000,
        "tolerance_percent": 1.0,
    }
    limits = counts.lowered("stress-255", found, old)
    assert limits["worst_frame_instructions"] == 60
    assert limits["total_instructions"] == 150
    assert "tolerance_percent" not in limits


def test_a_count_below_its_limit_past_the_tolerance_fails() -> None:
    # Room left under a limit lets a later change add the work back without failing.
    limits = {"worst_frame_instructions": 80, "total_instructions": 150}
    found = measured([10, 50, 70, 20])
    failed, _ = counts.judge(
        "stress-255", found, limits, RANKED, tolerance=1.0, fail_below=True
    )
    assert len(failed) == 1
    assert "worst_frame_instructions 70, -12.50% against the limit 80" in failed[0]
    assert "--counts --update" in failed[0]
    # During the build-out a cut passes, or every cut would file a bead.
    failed, said = counts.judge(
        "stress-255", found, limits, RANKED, tolerance=20.0, fail_below=False
    )
    assert failed == []
    assert len(said) == 2


def test_the_build_out_tolerance_passes_a_new_verb_and_fails_a_jump() -> None:
    limits = {"worst_frame_instructions": 100, "total_instructions": 200}
    failed, _ = counts.judge(
        "stress-255",
        measured([80, 115]),
        limits,
        (1,),
        tolerance=20.0,
        fail_below=False,
    )
    assert failed == []
    failed, _ = counts.judge(
        "stress-255",
        measured([80, 125]),
        limits,
        (1,),
        tolerance=20.0,
        fail_below=False,
    )
    assert len(failed) == 1
    assert "over the tolerance of 20.0%" in failed[0]


def board(frames: list[int]) -> counts.Counted:
    return counts.Counted(dict(enumerate(frames)))


def test_cost_growing_faster_than_the_board_fails_the_scaling_check() -> None:
    scaling = {
        "stress-1024": {
            "over": "stress-255",
            "worst_frame_instructions": 3.5,
            "total_instructions": 2.5,
        }
    }
    small = board([100, 300])
    # Four times the cards and twice the total: the fixed cost dominates, as it does today.
    linear = {"stress-255": small, "stress-1024": board([200, 600])}
    failed, said = counts.scaling_failures(linear, scaling)
    assert failed == []
    assert len(said) == 2
    # A search over every card for each moving card: the larger board costs far more.
    squared = {"stress-255": small, "stress-1024": board([300, 1050])}
    failed, _ = counts.scaling_failures(squared, scaling)
    assert len(failed) == 1
    assert "total_instructions 1,350 / 400 is 3.375 times" in failed[0]
    assert "faster than the board" in failed[0]
    # Counting only one of the pair (`--board`) checks nothing.
    assert counts.scaling_failures({"stress-255": small}, scaling) == ([], [])


def test_the_machine_script_counts_every_board_at_once_under_callgrind() -> None:
    script = counts.script(["stress-255", "pay-default"])
    # The program dumps each frame itself; Callgrind's own triggers missed returns on arm64.
    assert "--collect-atstart=no" in script
    assert "--dump-after" not in script
    assert "for board in stress-255 pay-default; do" in script
    assert vm.JOBS_SCRIPT in script
