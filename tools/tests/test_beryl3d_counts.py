"""`just b3d gate --counts`: reading Callgrind's per-frame counts of a board and holding them to
their limits."""

import pytest

from beryl_tools import calibration
from beryl_tools.beryl3d import counts, gate, vm

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


def budget(maximum: float) -> gate.Budget:
    return {"max": maximum, "why": "test budget"}


def test_the_worst_frame_fails_one_instruction_over_its_budget() -> None:
    budgets = {
        "worst_frame_instructions": budget(69),
        "total_instructions": budget(150),
    }
    failed, said = counts.judge(
        "stress-255", measured([10, 50, 70, 20]), budgets, RANKED
    )
    assert failed == [
        "stress-255 frame 2: worst_frame_instructions is 70, over its budget 69"
    ]
    assert said == ["stress-255 in total: total_instructions is 150, budget 150"]


def test_counts_below_their_budgets_pass() -> None:
    budgets = {
        "worst_frame_instructions": budget(100),
        "total_instructions": budget(200),
    }
    failed, said = counts.judge(
        "stress-255", measured([10, 50, 70, 20]), budgets, RANKED
    )
    assert failed == []
    assert len(said) == 2


def test_a_frame_the_counters_did_not_rank_fails() -> None:
    budgets = {
        "worst_frame_instructions": budget(90),
        "total_instructions": budget(220),
    }
    failed, _ = counts.judge("stress-255", measured([10, 50, 70, 90]), budgets, RANKED)
    assert len(failed) == 1
    assert "frame 3 has the most instructions (90)" in failed[0]


def board(frames: list[int]) -> counts.Counted:
    return counts.Counted(dict(enumerate(frames)))


def test_cost_growing_faster_than_the_board_fails_the_scaling_check() -> None:
    scaling: dict[str, gate.Scaling] = {
        "stress-1024": {
            "over": "stress-255",
            "limits": {
                "worst_frame_instructions": budget(3.5),
                "total_instructions": budget(2.5),
            },
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


def test_count_measurement_requires_matching_tools_and_every_native_frame(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile = calibration.Profile(
        "cygnus", "a" * 40, "beryl3d-frame-instructions/v1", {"backend": "wsl"}, {}
    )
    native = gate.Board(
        "stress-255",
        [gate.Frame(index, 16 * index, "rest", {}) for index in range(2)],
        0,
        None,
    )
    guest = output("stress-255", [3, 5])
    monkeypatch.setattr(
        counts, "measure", lambda *_a, **_kw: "runtime backend=lima\n" + guest
    )
    with pytest.raises(calibration.SetupError, match="incompatible.*backend"):
        counts.collect([native], profile)
    monkeypatch.setattr(
        counts,
        "measure",
        lambda *_a, **_kw: "runtime backend=wsl\n" + output("stress-255", [3]),
    )
    with pytest.raises(calibration.SetupError, match="every native frame"):
        counts.collect([native], profile)
    monkeypatch.setattr(
        counts, "measure", lambda *_a, **_kw: "runtime backend=wsl\n" + guest
    )
    assert counts.collect([native], profile)["stress-255"].total == 8


def test_an_instruction_override_cannot_borrow_another_verifiers_limits() -> None:
    budgets: gate.Budgets = {
        "boards": {
            "stress-255": {
                "worst_frame_instructions": budget(100),
                "total_instructions": budget(200),
            }
        },
        "enhanced": {},
        "scaling": {},
        "trend": {"adjacent_percent": 5, "weekly_percent": 10},
    }
    profile = calibration.Profile(
        "cygnus",
        "a" * 40,
        "beryl3d-frame-instructions/v1",
        {"backend": "wsl"},
        {"base": "budgets.json", "boards": {}},
    )
    with pytest.raises(
        calibration.SetupError, match="no instruction limits for stress-255"
    ):
        counts.budgets_for(budgets, profile)
