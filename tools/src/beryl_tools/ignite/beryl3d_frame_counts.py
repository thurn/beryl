"""List and run the calibrated Callgrind frame gate over every board together."""

from __future__ import annotations

import sys

from .. import calibration
from ..beryl3d import counts, frame_profiles, gate
from ..system import exe
from . import Listed, Result
from .beryl3d import DEBUG, cards_by_recording, gate_boards, pack_path, run_out

CHECK = "b3d-frame-counts:boards"


def list_checks() -> list[Listed]:
    cards = cards_by_recording()
    inputs = sorted({card for board in gate_boards() for card in cards.get(board, [])})
    digest = ""
    try:
        digest = counts.live_profile().digest
    except calibration.SetupError as error:
        print(f"b3d-frame-counts setup: {error}", file=sys.stderr)
    return [Listed(CHECK, inputs, digest=digest)]


def play() -> tuple[list[gate.Board], list[gate.Tiered]]:
    """Use the native gate and the recordings pack that Ignite already built."""
    done = run_out([str(exe(DEBUG / "gate"))])
    if done.returncode:
        raise ValueError(done.stderr.strip() or f"gate exited {done.returncode}")
    boards = gate.parse(done.stdout)
    enhanced = gate.parse_enhanced(done.stdout)
    names = gate_boards()
    if [board.name for board in boards] != names or [t.name for t in enhanced] != names:
        raise ValueError("gate did not play every board in both tiers")
    return boards, enhanced


def run(checks: list[str]) -> list[Result]:
    if CHECK not in checks:
        return []
    try:
        profile = frame_profiles.registered("instructions")
        budgets = counts.budgets_for(gate.read_budgets(), profile)
        boards, _ = play()
        found = counts.collect(boards, profile, pack=pack_path())
        failures, _ = counts.verdict(boards, found, budgets)
    except (OSError, ValueError, KeyError) as error:
        return [Result(CHECK, "incomplete", str(error))]
    if failures:
        return [Result(CHECK, "fail", "\n".join(failures))]
    return [Result(CHECK, "pass")]
