"""`just b3d gate --gpu`: the GPU's work, counted (docs/beryl-3d.md, 'What is counted'). GPU
time cannot be measured repeatably, so two kinds of work are counted instead, each held to
limits in `data/beryl3d/gates.json`:

- **Shader cost**, under `materials.<name>`: the texture samples (`samples`) and naga
  expressions (`expressions`) of each material's `shade_` function in beryl_render's shader,
  with the fragment entry point's added. The `shader_cost` test of beryl_render counts them and
  holds them to the limits, and runs on any machine, also in the lane's `cargo-test` step.
- **Fragments shaded**, under `tiers.baseline.fragments.<board>`: the most fragments any of a
  gated board's 8 ranked frames shades, in screens (fragments divided by the target's pixels).
  beryl_render's `fragments` example draws the frames in the `beryl3d-bench` virtual machine on
  Mesa's software Vulkan driver and reads the driver's count with a pipeline statistics query.
  It is exact, so the gate draws every frame twice and fails if the two counts differ.

`--update` lowers each limit to what was measured and sets a missing one; it never raises one.
"""

from __future__ import annotations

import json
import math
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass
from typing import Any

from .. import bench
from .. import paths as repo_paths
from . import counts, paths, source, vm

# The Baseline tier's target: no gated frame shades more than this many screens.
TARGET_SCREENS = 1.75
# Fragments in screens, kept to this many places and rounded up. On a 2880 by 1800 target one
# millionth of a screen is 5 fragments.
SCREEN_PLACES = 6
# The build directory in the machine, shared by every worktree's gate.
VM_TARGET = "$HOME/targets/gpu-gate"
# Where in the machine's home the recordings pack arrives, as `BERYL_CACHE` for the example.
VM_CACHE = "b3d-cache"
# The line the `shader_cost` test prints, after any progress dots `cargo test -q` writes.
MATERIALS_LINE: re.Pattern[str] = re.compile(r"materials (\{.*\})$", re.MULTILINE)


@dataclass(frozen=True)
class Counted:
    """One frame of a board, as the `fragments` example prints it."""

    frame: int
    ms: int
    phase: str
    fragments: int


@dataclass(frozen=True)
class BoardCount:
    name: str
    pixels: int
    frames: list[Counted]

    def worst(self) -> Counted:
        """The frame that shades the most, the earliest of equals."""
        return max(self.frames, key=lambda f: (f.fragments, -f.frame))

    def screens(self) -> float:
        return screens(self.worst().fragments, self.pixels)


def screens(fragments: int, pixels: int) -> float:
    """`fragments` as screens of `pixels`, rounded up to `SCREEN_PLACES` places."""
    scale = 10**SCREEN_PLACES
    return math.ceil(fragments * scale / pixels) / scale


def parse_boards(text: str) -> list[BoardCount]:
    return [
        BoardCount(
            name=str(b["name"]),
            pixels=int(b["pixels"]),
            frames=[
                Counted(
                    frame=int(f["frame"]),
                    ms=int(f["ms"]),
                    phase=str(f["phase"]),
                    fragments=int(f["fragments"]),
                )
                for f in b["frames"]
            ],
        )
        for b in json.loads(text)["boards"]
    ]


def parse_materials(text: str) -> dict[str, dict[str, int]]:
    """The counts the `shader_cost` test printed, by material."""
    found = MATERIALS_LINE.search(text)
    if found is None:
        return {}
    return {
        name: {k: int(v) for k, v in cost.items()}
        for name, cost in json.loads(found.group(1)).items()
    }


def fragment_failures(boards: list[BoardCount], limits: dict[str, float]) -> list[str]:
    lines: list[str] = []
    for board in boards:
        worst = board.worst()
        shown = board.screens()
        where = f"{board.name} frame {worst.frame} ({worst.ms} ms, {worst.phase})"
        limit = limits.get(board.name)
        if limit is None:
            lines.append(
                f"{where}: fragments is {shown} screens with no limit under"
                f" tiers.baseline.fragments; `just b3d gate --gpu --update` sets it"
            )
        elif shown > limit:
            lines.append(
                f"{where}: fragments is {shown} screens ({worst.fragments}), over its limit"
                f" {limit}"
            )
    return lines


def lowered_fragments(
    boards: list[BoardCount], limits: dict[str, float]
) -> dict[str, float]:
    updated = dict(limits)
    for board in boards:
        old = updated.get(board.name)
        updated[board.name] = (
            board.screens() if old is None else min(old, board.screens())
        )
    return updated


def lowered_materials(
    measured: dict[str, dict[str, int]], limits: dict[str, dict[str, int]]
) -> dict[str, dict[str, int]]:
    updated = {name: dict(cost) for name, cost in limits.items()}
    for name, cost in measured.items():
        material = updated.setdefault(name, {})
        for key, value in cost.items():
            old = material.get(key)
            material[key] = value if old is None else min(old, value)
    return updated


def shader_cost() -> tuple[int, str]:
    """Runs beryl_render's `shader_cost` test: its exit code and output."""
    from .cli import warm_target  # cli imports gate, which imports this module.

    warm_target()
    done = subprocess.run(
        [
            "cargo",
            "test",
            "-q",
            "-p",
            "beryl_render",
            "--test",
            "shader_cost",
            "--",
            "--nocapture",
            "--test-threads",
            "1",
        ],
        cwd=paths.WORKSPACE,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return done.returncode, done.stdout


def fragments_script(names: list[str]) -> str:
    """The script that takes in the sources (`source.py`), builds the `fragments` example from
    the machine's copy and runs it twice, reading the recordings pack that `send_pack` put in the
    machine (building it there would compile the engine, which exceeds 4 GiB). It holds the machine's lock so that it takes turns with the frame counts
    (counts.py): two builds at once exceed the machine's 4 GiB."""
    boards = " ".join(f"--board {shlex.quote(n)}" for n in names)
    return bench.locked(f""". "$HOME/.cargo/env"
set -e
{source.take_in_script()}cd {source.SOURCE}/rust/beryl3d
export CARGO_TARGET_DIR="{VM_TARGET}" BERYL_CACHE="$HOME/{VM_CACHE}"
cargo build --locked -q -j {counts.JOBS} -p beryl_render --example fragments
"$CARGO_TARGET_DIR/debug/examples/fragments" {boards}
"$CARGO_TARGET_DIR/debug/examples/fragments" {boards}
""")


def send_pack() -> bool:
    """Builds the recordings pack for the present sources on this machine, where the engine
    that plays them builds, and copies it into the VM's `BERYL_CACHE`. The pack is named for a
    hash of the sources, not of the checkout's path, so the example in the VM finds it.
    """
    built = subprocess.run(
        ["cargo", "run", "-q", "-p", "beryl_record", "--bin", "record"],
        cwd=paths.WORKSPACE,
        check=False,
        stdout=subprocess.PIPE,
        text=True,
    )
    lines = built.stdout.split()
    if built.returncode != 0 or not lines:
        return False
    directory = f"{VM_CACHE}/b3d"
    make = [
        "limactl",
        "shell",
        "--workdir",
        "/",
        vm.NAME,
        "bash",
        "-c",
        f'mkdir -p "$HOME/{directory}"',
    ]
    copy = ["limactl", "copy", lines[-1], f"{vm.NAME}:{directory}/"]
    return all(subprocess.run(cmd, check=False).returncode == 0 for cmd in (make, copy))


def count_fragments(names: list[str]) -> list[BoardCount]:
    """Each named board's (every gated board's for none) ranked frames counted in the machine. Fails unless both runs printed
    the same counts."""
    code = vm.ensure()
    if code != 0:
        raise SystemExit(code)
    if not send_pack():
        raise SystemExit(1)
    done = source.run_with_sources(fragments_script(names))
    if done is None:
        raise SystemExit(1)
    if done.returncode != 0:
        raise SystemExit(done.returncode)
    runs = [line for line in done.stdout.splitlines() if line.startswith("{")]
    if len(runs) != 2:
        print(f"FAIL the fragments example printed {len(runs)} results, not 2")
        raise SystemExit(1)
    first, second = (parse_boards(run) for run in runs)
    if first != second:
        print(
            "FAIL the fragment counts differ between two runs:\n"
            f"{runs[0]}\n{runs[1]}",
            file=sys.stderr,
        )
        raise SystemExit(1)
    adapter = json.loads(runs[0])["adapter"]
    print(f"fragments counted on {adapter}, the same on two runs")
    return first


def run_gpu(names: list[str], update: bool, gates: dict[str, Any]) -> int:
    """Both GPU gates for the boards `names`: 0 if every count is within its limit."""
    code, output = shader_cost()
    measured = parse_materials(output)
    if update and measured:
        gates["materials"] = lowered_materials(measured, gates.get("materials", {}))
        write(gates)
        code, output = shader_cost()
    for name, cost in sorted(measured.items()):
        print(
            f"material {name}: {cost['samples']} samples,"
            f" {cost['expressions']} expressions"
        )
    lines: list[str] = []
    if code != 0:
        lines.append(f"beryl_render's shader_cost test failed:\n{output}")

    boards = count_fragments(names)
    baseline = gates.setdefault("tiers", {}).setdefault("baseline", {})
    if update:
        baseline["fragments"] = lowered_fragments(boards, baseline.get("fragments", {}))
        write(gates)
    for board in boards:
        worst = board.worst()
        print(
            f"{board.name}: worst of {len(board.frames)} ranked frames is frame"
            f" {worst.frame}, {worst.fragments} fragments, {board.screens()} screens"
            f" (target {TARGET_SCREENS})"
        )
    lines += fragment_failures(boards, baseline.get("fragments", {}))
    for line in lines:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if lines else 0


def write(gates: dict[str, Any]) -> None:
    from .gate import GATES

    GATES.write_text(json.dumps(gates, indent=2) + "\n")
    print(f"wrote {GATES.relative_to(repo_paths.REPO)}")
