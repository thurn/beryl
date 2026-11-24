"""`just client gate --gpu` checks shader cost and the number of fragments shaded per frame.

The `shader_cost` test holds each material's texture samples and naga expressions to the
limits under `materials` in `data/client/gates.json`. It needs no GPU.

The `fragments` example draws each board's ranked frames on a software Vulkan driver in
`beryl3d-bench`. The gate requires two runs to agree, then checks each tier's fragment budget
in `data/client/budgets.json`. The measurement is in screens (fragments across all render
passes divided by the render target's pixels). Enhanced draw calls also have a budget.
"""

from __future__ import annotations

import json
import math
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass

from .. import bench
from . import gate, paths, source, vm

TIERS = ("baseline", "enhanced")
# Fragments in screens, kept to this many places and rounded up. On a 2880 by 1800 target one
# millionth of a screen is 5 fragments.
SCREEN_PLACES = 6
# The build directory in the machine, shared by every worktree's gate.
VM_TARGET = "$HOME/targets/gpu-gate"
# The line the `shader_cost` test prints, after any progress dots `cargo test -q` writes.
MATERIALS_LINE: re.Pattern[str] = re.compile(r"materials (\{.*\})$", re.MULTILINE)


@dataclass(frozen=True)
class Counted:
    """One frame of a board, as the `fragments` example prints it."""

    frame: int
    ms: int
    phase: str
    fragments: int
    draw_calls: int = 0


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
                    draw_calls=int(f.get("draw_calls", 0)),
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


def fragment_failures(
    boards: list[BoardCount], budgets: dict[str, dict[str, gate.Budget]], tier: str
) -> list[str]:
    """Each board whose worst ranked frame exceeds its fragment budget."""
    failed: list[str] = []
    key = f"{tier}_fragment_screens"
    for board in boards:
        worst = board.worst()
        shown = board.screens()
        budget = budgets.get(board.name, {}).get(key)
        line = (
            f"{board.name} frame {worst.frame} ({worst.ms} ms, {worst.phase}) in {tier}:"
            f" {key} is {shown} screens ({worst.fragments})"
        )
        if budget is None:
            failed.append(f"{line} with no budget")
        elif shown > budget["max"]:
            failed.append(f"{line}, over its budget {budget['max']}")
    return failed


def draw_call_failures(boards: list[BoardCount], limit: float | None) -> list[str]:
    """One line for each board with a ranked frame that takes more than `limit` draw calls
    in the Enhanced tier."""
    if limit is None:
        return ["no budget under enhanced.draw_calls"]
    lines: list[str] = []
    for board in boards:
        worst = max(board.frames, key=lambda f: (f.draw_calls, -f.frame))
        if worst.draw_calls > limit:
            lines.append(
                f"{board.name} frame {worst.frame} ({worst.ms} ms, {worst.phase}) in"
                f" enhanced: {worst.draw_calls} draw calls, over the budget {limit}"
            )
    return lines


def shader_cost() -> tuple[int, str]:
    """Runs beryl_render's `shader_cost` tests, in the crate's one test binary, `all`: their exit
    code and output."""
    done = subprocess.run(
        [
            "cargo",
            "test",
            "-q",
            "-p",
            "beryl_render",
            "--test",
            "all",
            "--",
            "shader_cost",
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
    the machine's copy and runs it twice in each tier, reading the recordings pack that `source.send_pack` put in
    the machine (building it there would compile the engine, which exceeds 4 GiB). It holds the machine's lock so that it takes turns with the frame counts
    (counts.py), since two builds at once could exceed the machine's memory."""
    boards = " ".join(f"--board {shlex.quote(n)}" for n in names)
    runs = "".join(
        f'"$CARGO_TARGET_DIR/debug/examples/fragments" --tier {tier} {boards}\n' * 2
        for tier in TIERS
    )
    setup = vm.provision_script(bench.toolchain())
    return setup + bench.locked(f""". "$HOME/.cargo/env"
set -e
{source.take_in_script()}cd {source.SOURCE}/client
export CARGO_TARGET_DIR="{VM_TARGET}" BERYL_CACHE="$HOME/{source.VM_CACHE}"
{vm.JOBS_SCRIPT}cargo build --locked -q -p beryl_pictures --example fragments
{runs}""")


def count_fragments(names: list[str]) -> dict[str, list[BoardCount]]:
    """Each named board's (every gated board's for none) ranked frames counted in the machine,
    by tier. Fails unless both runs of a tier printed the same counts."""
    code = vm.start()
    if code != 0:
        raise SystemExit(code)
    # The standalone panel scene is built from toolkit elements and reads no recordings.
    needs_pack = not names or any(name != "backdrop-panel" for name in names)
    if needs_pack and not source.send_pack():
        raise SystemExit(1)
    done = source.run_with_sources(fragments_script(names))
    if done is None:
        raise SystemExit(1)
    if done.returncode != 0:
        sys.stderr.write(done.stdout)
        raise SystemExit(done.returncode)
    runs = [line for line in done.stdout.splitlines() if line.startswith("{")]
    return compare_runs(runs)


def compare_runs(runs: list[str]) -> dict[str, list[BoardCount]]:
    """The counts by tier in the `fragments` example's outputs `runs`, two for each tier.
    Fails unless each tier's two runs printed the same counts."""
    by_tier: dict[str, list[str]] = {}
    for run in runs:
        by_tier.setdefault(str(json.loads(run)["tier"]), []).append(run)
    counted: dict[str, list[BoardCount]] = {}
    for tier in TIERS:
        found = by_tier.get(tier, [])
        if len(found) != 2:
            print(
                f"FAIL the fragments example printed {len(found)} {tier} results, not 2",
                file=sys.stderr,
            )
            raise SystemExit(1)
        first, second = (parse_boards(run) for run in found)
        if first != second:
            print(
                f"FAIL the {tier} fragment counts differ between two runs:\n"
                f"{found[0]}\n{found[1]}",
                file=sys.stderr,
            )
            raise SystemExit(1)
        counted[tier] = first
    adapter = json.loads(runs[0])["adapter"]
    print(
        f"fragments counted on {adapter}, the same on two runs in each tier",
        file=sys.stderr,
    )
    return counted


def run_gpu(names: list[str], budgets: gate.Budgets, *, json_output: bool) -> int:
    """Both GPU gates for the named boards, or every gated board."""
    code, output = shader_cost()
    materials = parse_materials(output)
    if not json_output:
        for name, cost in sorted(materials.items()):
            print(
                f"material {name}: {cost['samples']} samples,"
                f" {cost['expressions']} expressions"
            )
    lines: list[str] = []
    if code != 0:
        lines.append(f"beryl_render's shader_cost test failed:\n{output}")

    measurements: dict[str, dict[str, float]] = {}
    for tier, boards in count_fragments(names).items():
        for board in boards:
            worst = board.worst()
            calls = max(f.draw_calls for f in board.frames)
            shaded = board.screens()
            measurements.setdefault(board.name, {}).update(
                {
                    f"{tier}_worst_frame_fragments": worst.fragments,
                    f"{tier}_fragment_screens": shaded,
                    f"{tier}_worst_ranked_draw_calls": calls,
                }
            )
            if not json_output:
                print(
                    f"{board.name} {tier}: worst of {len(board.frames)} ranked frames is"
                    f" frame {worst.frame}, {worst.fragments} fragments, {shaded} screens;"
                    f" at most {calls} draw calls"
                )
        lines += fragment_failures(boards, budgets["boards"], tier)
        if tier == "enhanced":
            draw_calls = budgets["enhanced"].get("draw_calls")
            lines += draw_call_failures(
                boards, draw_calls["max"] if draw_calls else None
            )
    if json_output:
        print(json.dumps({"boards": measurements, "materials": materials}))
    for line in lines:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if lines else 0
