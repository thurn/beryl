"""`just b3d gate --gpu`: the GPU's work, counted (docs/beryl-3d.md, 'What is counted'). GPU
time cannot be measured repeatably, so two kinds of work are counted instead, each held to
limits in `data/beryl3d/gates.json`:

- **Shader cost**, under `materials.<name>`: the texture samples (`samples`) and naga
  expressions (`expressions`) of each material's `shade_` function in beryl_render's shader,
  with the fragment entry point's added. The `shader_cost` test of beryl_render counts them and
  holds them to the limits, and runs on any machine, also in `cargo-test`.
- **Fragments shaded**, under `tiers.<tier>.fragments.<board>` for the Baseline and Enhanced
  tiers: the most fragments any of a gated board's 8 ranked frames shades in that tier, in
  screens (fragments divided by the target's pixels), summed over every render pass of the
  frame. beryl_render's `fragments` example draws the frames in the `beryl3d-bench` virtual
  machine on Mesa's software Vulkan driver and reads the driver's count with pipeline
  statistics queries. It is exact, so the gate draws every frame twice and fails if the two
  counts differ. During the build-out (build_out.py) a board over its own limit but within
  its tier's target ([`TARGETS`]) is a warning. In Enhanced a board over the target always
  fails ([`HARD_TARGETS`]).
- **Draw calls in Enhanced**, under `tiers.enhanced.draw_calls`: the most draw calls any ranked
  frame of any board may take in the Enhanced tier, over all of its passes.

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
from . import build_out, paths, source, vm

# Each tier's target: no gated frame shades more than this many screens in that tier.
TARGETS: dict[str, float] = {"baseline": 1.75, "enhanced": 4.0}
TIERS: tuple[str, ...] = tuple(TARGETS)
# The tiers whose per-board limits must stay under the target, so that a board over it fails
# whatever its limit. Baseline's limits were set before the target was held this way, and its
# target only decides which boards fail during the build-out.
HARD_TARGETS: frozenset[str] = frozenset({"enhanced"})
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
    boards: list[BoardCount],
    limits: dict[str, float],
    *,
    warn_within_target: bool,
    tier: str = "baseline",
) -> tuple[list[str], list[str]]:
    """The failures and the warnings for each board's fragments in `tier`. A board over its
    limit fails, or with `warn_within_target` (the build-out) is only a warning unless it is
    over the tier's target too. A board over the target fails whatever its limit."""
    target = TARGETS[tier]
    failed: list[str] = []
    warned: list[str] = []
    for board in boards:
        worst = board.worst()
        shown = board.screens()
        where = f"{board.name} frame {worst.frame} ({worst.ms} ms, {worst.phase})"
        if tier != "baseline":
            where = f"{where} in {tier}"
        limit = limits.get(board.name)
        line = f"{where}: fragments is {shown} screens ({worst.fragments})"
        if limit is None:
            failed.append(
                f"{where}: fragments is {shown} screens with no limit under"
                f" tiers.{tier}.fragments; `just b3d gate --gpu --update` sets it"
            )
        elif shown > limit or (shown > target and tier in HARD_TARGETS):
            over = [f"over its limit {limit}"] if shown > limit else []
            over += [f"over the target {target}"] if shown > target else []
            line = f"{line}, {' and '.join(over)}"
            if warn_within_target and shown <= target:
                warned.append(f"{line}, within the target {target}")
            else:
                failed.append(line)
    return failed, warned


def draw_call_failures(boards: list[BoardCount], limit: int | None) -> list[str]:
    """One line for each board with a ranked frame that takes more than `limit` draw calls
    in the Enhanced tier."""
    if limit is None:
        return ["no limit under tiers.enhanced.draw_calls"]
    lines: list[str] = []
    for board in boards:
        worst = max(board.frames, key=lambda f: (f.draw_calls, -f.frame))
        if worst.draw_calls > limit:
            lines.append(
                f"{board.name} frame {worst.frame} ({worst.ms} ms, {worst.phase}) in"
                f" enhanced: {worst.draw_calls} draw calls, over the limit {limit}"
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
    return bench.locked(f""". "$HOME/.cargo/env"
set -e
{source.take_in_script()}cd {source.SOURCE}/rust/beryl3d
export CARGO_TARGET_DIR="{VM_TARGET}" BERYL_CACHE="$HOME/{source.VM_CACHE}"
{vm.JOBS_SCRIPT}cargo build --locked -q -p beryl_render --example fragments
{runs}""")


def count_fragments(names: list[str]) -> dict[str, list[BoardCount]]:
    """Each named board's (every gated board's for none) ranked frames counted in the machine,
    by tier. Fails unless both runs of a tier printed the same counts."""
    code = vm.ensure()
    if code != 0:
        raise SystemExit(code)
    if not source.send_pack():
        raise SystemExit(1)
    done = source.run_with_sources(fragments_script(names))
    if done is None:
        raise SystemExit(1)
    if done.returncode != 0:
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
                f"FAIL the fragments example printed {len(found)} {tier} results, not 2"
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
    print(f"fragments counted on {adapter}, the same on two runs in each tier")
    return counted


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

    tiers = gates.setdefault("tiers", {})
    for tier, boards in count_fragments(names).items():
        held = tiers.setdefault(tier, {})
        if update:
            held["fragments"] = lowered_fragments(boards, held.get("fragments", {}))
            write(gates)
        for board in boards:
            worst = board.worst()
            calls = max(f.draw_calls for f in board.frames)
            print(
                f"{board.name} {tier}: worst of {len(board.frames)} ranked frames is"
                f" frame {worst.frame}, {worst.fragments} fragments, {board.screens()}"
                f" screens (target {TARGETS[tier]}); at most {calls} draw calls"
            )
        failed, warned = fragment_failures(
            boards,
            held.get("fragments", {}),
            warn_within_target=build_out.BUILD_OUT,
            tier=tier,
        )
        lines += failed
        if tier == "enhanced":
            lines += draw_call_failures(boards, held.get("draw_calls"))
        for line in warned:
            print(f"WARN {line}")
    for line in lines:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if lines else 0


def write(gates: dict[str, Any]) -> None:
    from .gate import GATES

    GATES.write_text(json.dumps(gates, indent=2) + "\n")
    print(f"wrote {GATES.relative_to(repo_paths.REPO)}")
