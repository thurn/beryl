"""`just b3d gate --counts`: the instruction-count gate for frames (docs/beryl-3d.md, 'Finding
the worst frame'). It counts the gated boards' frames under Callgrind in `beryl3d-bench`, with
the Gungraun benchmark `beryl_inspect/benches/frame_counts.rs`, and holds each board to the
limits in `data/beryl3d/gates.json` under `frames.<board>`:

- `worst_frame_instructions`: the most instructions in one frame (its `handle` calls and its
  `frame` call), the largest of the counts of the frames `rank` picks from the work counters.
- `total_instructions`: the whole playback's.
- `tolerance_percent`: how far a count may pass its limit. Counts are exact, so this is not for
  noise: it absorbs the small shifts when an unrelated change alters how the compiler inlines
  and lays out this code, as the engine's count gate does.

It also checks the ranking: of the frames of the board's stretch, each counted on its own, the
one with the most instructions must be among the ranked frames.

`--update` lowers each limit to what was measured and never raises one. A limit rises only by
hand, when the work itself changed or a compiler or dependency upgrade moved the count, with the
old and new numbers in the commit body (rust/beryl3d/AGENTS.md).

The machine mounts only the main checkout, and the sweep checks out its commit elsewhere, so
the sources the benchmark needs are sent into the machine (`source.py`).
"""

from __future__ import annotations

import json
import platform
import shlex
import shutil
import sys
from dataclasses import dataclass
from typing import Any

from .. import bench
from .. import paths as repo_paths
from . import gate, source, vm

# In the machine: the build directory and Gungraun home the gate keeps warm between runs.
TARGET = "$HOME/targets/b3d-gate"
HOME = "$HOME/gungraun/b3d-gate"
# One compiler at a time: two of the main workspace's crates built at once pass the machine's
# 4 GiB, and the kernel kills one.
JOBS = "1"
BENCH = "frame_counts"
# Gungraun's names for the benchmark's functions (`<module path>::<function>`).
GROUP = f"{BENCH}::frames"
TOLERANCE = 1.0


@dataclass(frozen=True)
class Counted:
    """A board's instruction counts: its whole playback, each ranked frame and each frame of
    its stretch, by frame index."""

    total: int
    ranked: dict[int, int]
    stretch: dict[int, int]


def id_name(board: str) -> str:
    """A board's name as a Gungraun benchmark id: `stress-255` is `stress_255`."""
    return board.replace("-", "_")


def summaries(stdout: str) -> dict[str, int]:
    """Each benchmark's instructions (Callgrind's `Ir`) from Gungraun's JSON lines, by
    `<module path>::<id>`."""
    found: dict[str, int] = {}
    for line in stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            summary = json.loads(line)
        except json.JSONDecodeError:
            continue
        for profile in summary.get("profiles", []):
            if profile.get("tool") == "Callgrind":
                metrics = profile["data"]["total"]["metrics"]
                key = f"{summary.get('module_path')}::{summary.get('id')}"
                found[key] = int(metrics["Ir"]["values"]["new"])
    return found


def counted(board: gate.Board, found: dict[str, int]) -> Counted | None:
    """`board`'s counts among `found`, or `None` when Gungraun reported one of them missing.
    Run `n` of a function's `benches` for the board counts the `n`th of its frames."""
    name = id_name(board.name)
    total = found.get(f"{GROUP}::total::{name}")
    ranked: dict[int, int] = {}
    for place, frame in enumerate(board.ranked):
        count = found.get(f"{GROUP}::ranked::{name}_{place}")
        if count is None:
            return None
        ranked[frame] = count
    stretch: dict[int, int] = {}
    for place, frame in enumerate(range(*board.stretch)):
        count = found.get(f"{GROUP}::stretch::{name}_{place}")
        if count is None:
            return None
        stretch[frame] = count
    if total is None or not ranked or not stretch:
        return None
    return Counted(total, ranked, stretch)


def worst(counts: Counted) -> tuple[int, int]:
    """The ranked frame with the most instructions and its count, the earliest of equals."""
    frame = max(counts.ranked, key=lambda f: (counts.ranked[f], -f))
    return frame, counts.ranked[frame]


def judge(
    name: str, counts: Counted, limits: dict[str, Any]
) -> tuple[list[str], list[str]]:
    """The failures and the report lines for one board."""
    failed: list[str] = []
    said: list[str] = []
    tolerance = float(limits.get("tolerance_percent", TOLERANCE))
    frame, most = worst(counts)
    for key, value, where in (
        ("worst_frame_instructions", most, f"frame {frame}"),
        ("total_instructions", counts.total, "in total"),
    ):
        limit = limits.get(key)
        if limit is None:
            failed.append(
                f"{name} {where}: {key} is {value:,} with no limit;"
                " `just b3d gate --counts --update` sets it"
            )
            continue
        change = 100.0 * (value - limit) / limit
        line = f"{name} {where}: {key} {value:,}, {change:+.2f}% against the limit {limit:,}"
        if change > tolerance:
            failed.append(f"{line}, over the tolerance of {tolerance}%")
        elif change < -tolerance:
            said.append(f"{line}; `just b3d gate --counts --update` lowers it")
        else:
            said.append(line)
    top = max(counts.stretch, key=lambda f: (counts.stretch[f], -f))
    if top not in counts.ranked:
        first, end = min(counts.stretch), max(counts.stretch) + 1
        failed.append(
            f"{name}: frame {top} has the most instructions of frames {first} to {end - 1}"
            f" ({counts.stretch[top]:,}) but the work counters did not rank it among"
            f" {sorted(counts.ranked)}; make the counters see the work it does"
        )
    return failed, said


def lowered(name: str, counts: Counted, limits: dict[str, Any]) -> dict[str, Any]:
    """`limits` with each instruction limit lowered to what was measured, or set from it when
    missing, and the tolerance set when missing. No limit rises."""
    updated = dict(limits)
    _, most = worst(counts)
    for key, value in (
        ("worst_frame_instructions", most),
        ("total_instructions", counts.total),
    ):
        old = updated.get(key)
        updated[key] = value if old is None else min(int(old), value)
    updated.setdefault("tolerance_percent", TOLERANCE)
    return updated


def script(boards: list[str], runner: str) -> str:
    """Provisions the machine, takes in the sources (`source.take_in_script`), and builds and runs the
    benchmark for `boards`, printing Gungraun's JSON lines."""
    # Gungraun takes one filter, so several boards run every benchmark.
    filters = [f"{GROUP}::*::{id_name(boards[0])}*"] if len(boards) == 1 else []
    cargo = [
        "cargo",
        "bench",
        "--locked",
        "-j",
        JOBS,
        "-p",
        "beryl_inspect",
        "--bench",
        BENCH,
        "--",
        "--output-format=json",
        *filters,
    ]
    work = f"""{source.take_in_script()}cd {source.SOURCE}/rust/beryl3d
CARGO_TARGET_DIR={TARGET} GUNGRAUN_HOME={HOME} {shlex.join(cargo)}
"""
    return vm.provision_script(bench.toolchain(), runner) + bench.locked(work)


def measure(boards: list[str]) -> str | None:
    """Gungraun's output for `boards`, or `None` when the machine could not count."""
    if platform.machine() not in ("arm64", "aarch64"):
        bench.log("the frame counts are for Apple silicon's arm64; this machine is not")
        return None
    if shutil.which("limactl") is None:
        bench.log("Lima is missing: brew install lima, then `just b3d vm` once")
        return None
    if bench.vm_ensure(vm.NAME) != 0:
        return None
    runner = vm.runner_version()
    if runner is None:
        bench.log("no gungraun in rust/beryl3d/Cargo.lock")
        return None
    run = source.run_with_sources(script(boards, runner))
    if run is None or run.returncode != 0:
        return None
    return run.stdout


def run(boards: list[gate.Board], update: bool) -> int:
    stdout = measure([b.name for b in boards])
    if stdout is None:
        return 2
    found = summaries(stdout)
    data = json.loads(gate.GATES.read_text())
    frames: dict[str, dict[str, Any]] = data.setdefault("frames", {})
    measured: list[tuple[gate.Board, Counted]] = []
    for board in boards:
        counts = counted(board, found)
        if counts is None:
            bench.log(f"Gungraun reported no counts for {board.name}")
            return 2
        measured.append((board, counts))
    if update:
        for board, counts in measured:
            frames[board.name] = lowered(board.name, counts, frames.get(board.name, {}))
        gate.GATES.write_text(json.dumps(data, indent=2) + "\n")
        print(f"wrote {gate.GATES.relative_to(repo_paths.REPO)}")
    failed = False
    for board, counts in measured:
        failures, said = judge(board.name, counts, frames.get(board.name, {}))
        for line in said:
            print(line)
        for line in failures:
            print(f"FAIL {line}", file=sys.stderr)
        failed |= bool(failures)
    return 1 if failed else 0
