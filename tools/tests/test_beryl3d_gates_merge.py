"""gates_merge.py: the merge driver that reapplies each branch's change to a limit in
data/beryl3d/gates.json onto the other's."""

import json
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools.beryl3d import gates_merge


def gates(total: int, worst: int, area: float, name: str = "verbs-zones") -> str:
    board = {
        "worst_frame_records": worst,
        "total_records": total,
        "worst_frame_blended_area": area,
        "tolerance_percent": 1.0,
    }
    data = {"audit": {"step_ms": 150}, "frames": {name: board}}
    return json.dumps(data, indent=2) + "\n"


def test_both_sides_raising_a_limit_merge_to_the_sum_of_both_increases() -> None:
    base = gates(total=1000, worst=50, area=0.04)
    ours = gates(total=1300, worst=50, area=0.0436)
    theirs = gates(total=1050, worst=60, area=0.041)
    merged = json.loads(gates_merge.merge(base, ours, theirs))
    board = merged["frames"]["verbs-zones"]
    assert board["total_records"] == 1350
    # Only theirs moved it.
    assert board["worst_frame_records"] == 60
    # Decimal arithmetic: 0.0436 + 0.041 - 0.04, written without float noise.
    assert board["worst_frame_blended_area"] == 0.0446
    assert board["tolerance_percent"] == 1.0


def test_a_board_one_side_adds_is_kept_and_the_text_matches_the_gate_writer() -> None:
    base = gates(total=1000, worst=50, area=0.04)
    ours = gates(total=1100, worst=50, area=0.04)
    data = json.loads(base)
    data["frames"]["pay-default"] = {"total_records": 7}
    theirs = json.dumps(data, indent=2) + "\n"
    merged = gates_merge.merge(base, ours, theirs)
    expected = json.loads(ours)
    expected["frames"]["pay-default"] = {"total_records": 7}
    assert merged == json.dumps(expected, indent=2) + "\n"


def test_a_value_both_sides_change_that_is_not_a_number_is_a_conflict() -> None:
    base = json.dumps({"tests": {"note": "a"}})
    with pytest.raises(gates_merge.Conflict, match="tests.note"):
        gates_merge.merge(
            base,
            json.dumps({"tests": {"note": "b"}}),
            json.dumps({"tests": {"note": "c"}}),
        )
    # One side removed what the other changed.
    with pytest.raises(gates_merge.Conflict, match="tests.note"):
        gates_merge.merge(
            base, json.dumps({"tests": {}}), json.dumps({"tests": {"note": "c"}})
        )


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, capture_output=True, text=True, check=True
    ).stdout


def test_two_branches_rebase_onto_each_other_with_both_increases(
    tmp_path: Path,
) -> None:
    """Two branches cut from one commit, each raising total_records by its own amount, rebase
    with no conflict, through the driver as git runs it."""
    repo = tmp_path
    git(repo, "init", "-q", "-b", "staging")
    git(repo, "config", "user.email", "test@example.com")
    git(repo, "config", "user.name", "Test")
    (repo / ".gitattributes").write_text("gates.json merge=b3d-gates\n")
    script = Path(gates_merge.__file__)
    git(
        repo,
        "config",
        "merge.b3d-gates.driver",
        f"{sys.executable} {script} %O %A %B %P",
    )
    target = repo / "gates.json"
    target.write_text(gates(total=1000, worst=50, area=0.04))
    git(repo, "add", ".")
    git(repo, "commit", "-q", "-m", "base")
    git(repo, "checkout", "-q", "-b", "arrows")
    target.write_text(gates(total=2669, worst=50, area=0.04))
    git(repo, "commit", "-q", "-am", "arrows: +1669")
    git(repo, "checkout", "-q", "staging")
    target.write_text(gates(total=1200, worst=55, area=0.04))
    git(repo, "commit", "-q", "-am", "streak: +200")
    git(repo, "checkout", "-q", "arrows")
    git(repo, "rebase", "-q", "staging")
    board = json.loads(target.read_text())["frames"]["verbs-zones"]
    assert board["total_records"] == 2869
    assert board["worst_frame_records"] == 55


def test_a_conflict_leaves_git_conflict_markers(tmp_path: Path) -> None:
    base, ours, theirs = (tmp_path / n for n in ("base", "ours", "theirs"))
    base.write_text('{\n  "note": "a"\n}\n')
    ours.write_text('{\n  "note": "b"\n}\n')
    theirs.write_text('{\n  "note": "c"\n}\n')
    assert gates_merge.main([str(base), str(ours), str(theirs), "gates.json"]) == 1
    assert "<<<<<<< gates.json (ours)" in ours.read_text()
