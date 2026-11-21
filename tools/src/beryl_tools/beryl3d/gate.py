"""Play the gated boards and check their frame costs against `data/beryl3d/budgets.json`.

The native gate checks each work counter's worst frame and playback total, the worst Baseline
draw load, and the allocations made by `Client::handle`. Allocations inside `Client::frame`
always fail. The Enhanced playback also checks the particle budget and compares its resolved
element count with Baseline at each frame.

`--counts` checks instructions under Callgrind and `--gpu` checks fragments and shader cost.
`--json` reports the measurements from the selected mode for recording the trend.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TypedDict

from .. import paths as repo_paths
from . import paths

BUDGETS: Path = repo_paths.REPO / "data" / "beryl3d" / "budgets.json"


class Budget(TypedDict):
    max: float
    why: str


class Scaling(TypedDict):
    over: str
    limits: dict[str, Budget]


class Budgets(TypedDict):
    boards: dict[str, dict[str, Budget]]
    enhanced: dict[str, Budget]
    scaling: dict[str, Scaling]


def read_budgets() -> Budgets:
    return json.loads(BUDGETS.read_text())


# The client core's work counters (`beryl_scene::frame::Work`).
COUNTERS: tuple[str, ...] = (
    "elements",
    "segments",
    "records",
    "layout_nodes",
    "text_runs",
    "placed",
    "steps",
)
# The draw list's load (`beryl_scene::frame::Load`) held to a budget.
LOAD = ("draw_calls", "upload_bytes", "blended_area")


@dataclass(frozen=True)
class Frame:
    """One frame of a board's playback, as the `gate` binary prints it."""

    index: int
    ms: int
    phase: str
    counts: dict[str, float]

    def moment(self) -> str:
        return f"frame {self.index} ({self.ms} ms, {self.phase})"


@dataclass(frozen=True)
class Board:
    name: str
    frames: list[Frame]
    handle_allocations: int
    # The frame and backtrace of the first allocation inside `Client::frame`, if any.
    first_allocation: tuple[int, str] | None
    # The frames `rank` picks, most work first. The instruction-count gate (counts.py) counts
    # every frame and expects the one with the most instructions among these.
    ranked: tuple[int, ...] = ()


@dataclass(frozen=True)
class Tiered:
    """A board played in the Enhanced tier: each frame's particles and resolved elements, by
    frame."""

    name: str
    frames: list[Frame]


@dataclass(frozen=True)
class Measured:
    """A board's measurement of one limit: the number, and the frame it came from when it is
    one frame's."""

    key: str
    value: float
    frame: Frame | None


def parse(text: str) -> list[Board]:
    """The boards in the `gate` binary's output."""
    boards: list[Board] = []
    for board in json.loads(text)["boards"]:
        frames = [
            Frame(
                index=int(f["frame"]),
                ms=int(f["ms"]),
                phase=str(f["phase"]),
                counts={
                    k: float(v)
                    for k, v in f.items()
                    if k not in ("frame", "ms", "phase")
                },
            )
            for f in board["frames"]
        ]
        first = board["first_frame_allocation"]
        boards.append(
            Board(
                name=str(board["name"]),
                frames=frames,
                handle_allocations=int(board["handle_allocations"]),
                first_allocation=(
                    (int(first["frame"]), str(first["backtrace"])) if first else None
                ),
                ranked=tuple(int(f) for f in board.get("ranked", ())),
            )
        )
    return boards


def parse_enhanced(text: str) -> list[Tiered]:
    """The Enhanced playbacks in the `gate` binary's output, named without their suffix."""
    return [
        Tiered(
            name=str(board["name"]).removesuffix(" enhanced"),
            frames=[
                Frame(
                    index=int(f["frame"]),
                    ms=int(f["ms"]),
                    phase=str(f["phase"]),
                    counts={
                        "particles": float(f["particles"]),
                        "resolved": float(f["resolved"]),
                    },
                )
                for f in board["frames"]
            ],
        )
        for board in json.loads(text).get("enhanced", [])
    ]


def enhanced_failures(
    boards: list[Board], enhanced: list[Tiered], particles: float | None
) -> list[str]:
    """One line for each board whose worst Enhanced frame has more than `particles`, and for
    each Enhanced frame whose resolved elements differ in number from Baseline's at the same
    frame."""
    lines: list[str] = []
    baseline = {board.name: board for board in boards}
    for tiered in enhanced:
        name = f"{tiered.name} enhanced"
        if particles is None:
            lines.append(f"{name}: no budget under enhanced.particles")
        elif tiered.frames:
            m = worst(tiered.frames, "particles")
            assert m.frame
            if m.value > particles:
                lines.append(
                    f"{name} {m.frame.moment()}: particles is {int(m.value)}, over its"
                    f" budget {particles}"
                )
        board = baseline.get(tiered.name)
        if board is None:
            continue
        if len(board.frames) != len(tiered.frames):
            lines.append(
                f"{name}: {len(tiered.frames)} frames, but Baseline played"
                f" {len(board.frames)}"
            )
        for ours, theirs in zip(tiered.frames, board.frames):
            mine, base = ours.counts["resolved"], theirs.counts.get("resolved")
            if mine != base:
                lines.append(
                    f"{name} {ours.moment()}: {int(mine)} resolved elements, but Baseline"
                    f" has {base if base is None else int(base)}"
                )
                break
    return lines


def worst(frames: list[Frame], count: str) -> Measured:
    """The frame with the most of `count`, the earliest of equals."""
    frame = max(frames, key=lambda f: (f.counts[count], -f.index))
    return Measured(f"worst_frame_{count}", frame.counts[count], frame)


def measure(board: Board) -> list[Measured]:
    """Every number the gate holds for `board`, in the order it reports them."""
    measured: list[Measured] = []
    for counter in COUNTERS:
        measured.append(worst(board.frames, counter))
        total = sum(f.counts[counter] for f in board.frames)
        measured.append(Measured(f"total_{counter}", total, None))
    measured.extend(worst(board.frames, load) for load in LOAD)
    measured.append(
        Measured("handle_allocations", float(board.handle_allocations), None)
    )
    allocating = worst(board.frames, "frame_allocations")
    measured.append(Measured("frame_allocations", allocating.value, allocating.frame))
    return measured


def number(key: str, value: float) -> float:
    """Blended area is measured in screens. Every other native count is an integer."""
    return value if key.endswith("blended_area") else int(value)


def failures(boards: list[Board], budgets: dict[str, dict[str, Budget]]) -> list[str]:
    """One line for each exceeded or missing budget and for any frame allocations."""
    lines: list[str] = []
    for board in boards:
        board_budgets = budgets.get(board.name, {})
        for m in measure(board):
            where = f"{board.name} {m.frame.moment() if m.frame else 'in total'}"
            shown = number(m.key, m.value)
            if m.key == "frame_allocations":
                if shown > 0:
                    lines.append(f"{where}: frame_allocations is {shown}, must be zero")
                continue
            budget = board_budgets.get(m.key)
            if budget is None:
                lines.append(f"{where}: {m.key} is {shown} with no budget")
            elif shown > budget["max"]:
                lines.append(
                    f"{where}: {m.key} is {shown}, over its budget {budget['max']}"
                )
        if board.first_allocation:
            frame, backtrace = board.first_allocation
            lines.append(
                f"{board.name} frame {frame}: the first allocation inside Client::frame"
                f" was made at\n{backtrace}"
            )
    return lines


def report(boards: list[Board], enhanced: list[Tiered]) -> dict[str, dict[str, float]]:
    """One map of measurements per board, including counters without a budget."""
    measurements: dict[str, dict[str, float]] = {
        board.name: {
            **{m.key: number(m.key, m.value) for m in measure(board)},
            "worst_frame_instances": int(worst(board.frames, "instances").value),
            "worst_frame_resolved": int(worst(board.frames, "resolved").value),
            "frames": len(board.frames),
        }
        for board in boards
    }
    for tiered in enhanced:
        measurements[tiered.name].update(
            enhanced_worst_frame_particles=int(worst(tiered.frames, "particles").value),
            enhanced_worst_frame_resolved=int(worst(tiered.frames, "resolved").value),
        )
    return measurements


def summary(board: Board) -> str:
    """One line for `board`: its frames, and the worst frame of each work counter."""
    parts = []
    for counter in COUNTERS:
        m = worst(board.frames, counter)
        assert m.frame
        parts.append(f"{counter} {number(counter, m.value)} at {m.frame.index}")
    return f"{board.name}: {len(board.frames)} frames; worst " + ", ".join(parts)


def play(names: list[str], *, json_output: bool) -> tuple[list[Board], list[Tiered]]:
    """Plays the boards called `names` (every gated board for none) with the `gate` binary, in
    Baseline and in Enhanced."""
    from .cli import program, record  # cli imports this module.

    # The gated recorded boards are read from the pack, which only `record` builds. It builds
    # `gate` too, in the build the tests share.
    recorded = record(stdout=sys.stderr if json_output else None)
    if recorded != 0:
        raise SystemExit(recorded)
    command = [str(program("gate"))]
    for name in names:
        command += ["--board", name]
    done = subprocess.run(
        command,
        cwd=paths.WORKSPACE,
        check=False,
        stdout=subprocess.PIPE,
        text=True,
    )
    if done.returncode != 0:
        raise SystemExit(done.returncode)
    return parse(done.stdout), parse_enhanced(done.stdout)


def run_gate(args: argparse.Namespace) -> int:
    budgets = read_budgets()
    if args.gpu:
        from . import gpu  # It imports `bench`, which Ignite's adapter must not load.

        return gpu.run_gpu(list(args.board), budgets, json_output=args.json)
    boards, enhanced = play(list(args.board), json_output=args.json)
    if args.counts:
        from . import counts  # It imports this module.

        return counts.run(boards, budgets, json_output=args.json)
    particle_budget = budgets["enhanced"].get("particles")
    particles = particle_budget["max"] if particle_budget else None
    lines = failures(boards, budgets["boards"])
    lines += enhanced_failures(boards, enhanced, particles)
    if args.json:
        print(json.dumps({"boards": report(boards, enhanced)}))
    else:
        for board in boards:
            print(summary(board))
        for tiered in enhanced:
            m = worst(tiered.frames, "particles")
            assert m.frame
            print(
                f"{tiered.name} enhanced: worst particles {int(m.value)} at {m.frame.index}"
                f" (budget {particles})"
            )
    for line in lines:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if lines else 0


def gate_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--board",
        action="append",
        default=[],
        help="play only this board (repeatable); every gated board by default",
    )
    parser.add_argument(
        "--json", action="store_true", help="print the measurements as JSON"
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--gpu",
        action="store_true",
        help="count shader cost and the fragments each board shades in beryl3d-bench",
    )
    mode.add_argument(
        "--counts",
        action="store_true",
        help="hold instructions per frame instead, counted under Callgrind in beryl3d-bench",
    )
