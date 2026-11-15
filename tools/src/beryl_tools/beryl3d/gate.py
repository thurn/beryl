"""`just b3d gate`: the native frame gate (docs/beryl-3d.md, 'Performance gates'). It plays the
gated boards with the `gate` binary of `beryl_inspect` and fails on any count over its limit in
`data/beryl3d/gates.json` under `frames.<board>`, naming the board, the frame and the count.

The limits on each board:

- `worst_frame_<counter>` and `total_<counter>` for each of the client core's work counters
  (`COUNTERS`): the most in one frame, and the sum over the playback.
- `worst_frame_<measure>` for the draw list's load (`LOAD`): draw calls, upload bytes and
  blended area in screens, for the Baseline tier.
- `handle_allocations`: the heap allocations made in `Client::handle` over the playback.
- `frame_allocations`: those made in any `Client::frame`, which is always 0.

Every count is exact and the same on every run, so a count over its limit by one fails.
`--update` lowers each limit to what was measured and never raises one. A limit rises only by
hand, when the work itself changed or a compiler or dependency upgrade moved the count, with the
old and new numbers in the commit body (rust/beryl3d/AGENTS.md).

`--gpu` counts the GPU's work instead, the shader cost and fragments under `materials` and
`tiers.baseline` (gpu.py).
"""

from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import paths as repo_paths
from . import gpu, paths

GATES: Path = repo_paths.REPO / "data" / "beryl3d" / "gates.json"
# The client core's work counters (`beryl_scene::frame::Work`).
COUNTERS: tuple[str, ...] = (
    "elements",
    "segments",
    "records",
    "layout_nodes",
    "text_runs",
)
# The draw list's load (`beryl_scene::frame::Load`) and the limit each starts from before its
# first measurement lowers it.
LOAD: dict[str, float] = {
    "draw_calls": 16,
    "upload_bytes": 327_680,
    "blended_area": 0.75,
}
# Blended area is a fraction of a screen, kept to this many decimal places and rounded up, so
# that the measurement itself is within the limit written from it.
AREA_PLACES = 4


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
            )
        )
    return boards


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


def number(key: str, value: float) -> int | float:
    """`value` as gates.json keeps it: blended area rounded up to `AREA_PLACES` places, every
    other count a whole number."""
    if key.endswith("blended_area"):
        scale = 10**AREA_PLACES
        return math.ceil(round(value * scale, 6)) / scale
    return int(value)


def failures(boards: list[Board], limits: dict[str, dict[str, Any]]) -> list[str]:
    """One line for each number over its limit or with no limit, and the backtrace of a frame's
    first allocation."""
    lines: list[str] = []
    for board in boards:
        board_limits = limits.get(board.name, {})
        for m in measure(board):
            limit = board_limits.get(m.key)
            where = f"{board.name} {m.frame.moment() if m.frame else 'in total'}"
            shown = number(m.key, m.value)
            if limit is None:
                lines.append(
                    f"{where}: {m.key} is {shown} with no limit;"
                    " `just b3d gate --update` sets it"
                )
            elif number(m.key, m.value) > limit:
                lines.append(f"{where}: {m.key} is {shown}, over its limit {limit}")
        if board.first_allocation:
            frame, backtrace = board.first_allocation
            lines.append(
                f"{board.name} frame {frame}: the first allocation inside Client::frame"
                f" was made at\n{backtrace}"
            )
    return lines


def lowered(
    boards: list[Board], limits: dict[str, dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    """`limits` with each lowered to what `boards` measured. A missing limit is set from the
    measurement, or for the load from its starting limit if that is lower. `frame_allocations`
    is always 0, and no limit rises."""
    updated = {name: dict(board) for name, board in limits.items()}
    for board in boards:
        board_limits = updated.setdefault(board.name, {})
        for m in measure(board):
            value = number(m.key, m.value)
            if m.key == "frame_allocations":
                board_limits[m.key] = 0
                continue
            start = LOAD.get(m.key.removeprefix("worst_frame_"))
            old = board_limits.get(m.key, start)
            board_limits[m.key] = value if old is None else min(old, value)
    return updated


def summary(board: Board) -> str:
    """One line for `board`: its frames, and the worst frame of each work counter."""
    parts = []
    for counter in COUNTERS:
        m = worst(board.frames, counter)
        assert m.frame
        parts.append(f"{counter} {number(counter, m.value)} at {m.frame.index}")
    return f"{board.name}: {len(board.frames)} frames; worst " + ", ".join(parts)


def play(names: list[str]) -> list[Board]:
    """Plays the boards called `names` (every gated board for none) with the `gate` binary."""
    from .cli import warm_target  # cli imports this module.

    warm_target()
    command = ["cargo", "run", "-q", "-p", "beryl_inspect", "--bin", "gate", "--"]
    for name in names:
        command += ["--board", name]
    done = subprocess.run(
        command,
        cwd=paths.WORKSPACE,
        env=os.environ,
        check=False,
        stdout=subprocess.PIPE,
        text=True,
    )
    if done.returncode != 0:
        raise SystemExit(done.returncode)
    return parse(done.stdout)


def run_gate(args: argparse.Namespace) -> int:
    data = json.loads(GATES.read_text())
    if args.gpu:
        return gpu.run_gpu(list(args.board), args.update, data)
    boards = play(list(args.board))
    if args.update:
        data["frames"] = lowered(boards, data.get("frames", {}))
        GATES.write_text(json.dumps(data, indent=2) + "\n")
        print(f"wrote {GATES.relative_to(repo_paths.REPO)}")
    for board in boards:
        print(summary(board))
    lines = failures(boards, data.get("frames", {}))
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
        "--update",
        action="store_true",
        help="lower each limit to what was measured; never raises one",
    )
    parser.add_argument(
        "--gpu",
        action="store_true",
        help="count the GPU's work instead: each material's shader cost, and the fragments"
        " each board shades, drawn in the beryl3d-bench machine (gpu.py)",
    )
