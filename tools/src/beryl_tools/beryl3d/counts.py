"""`just b3d gate --counts` counts every frame under Callgrind in `beryl3d-bench`.
For each board it checks the worst frame (its `handle` calls and its `frame` call) and the
total instructions against `data/beryl3d/budgets.json`.

The scaling check compares `stress-1024` with `stress-255`. Both boards play the same table,
with four times as many cards on the larger board. Its counts can grow by at most the ratio
measured when the work was linear, plus headroom.

The frame with the most instructions must be among the frames `rank` picks from the native work
counters, so the native gate keeps seeing the work that costs instructions.

The machine mounts only the main checkout, and a worktree is elsewhere, so
the sources the program needs are sent into the machine (`source.py`), with the recordings
pack the recorded boards are read from.
"""

from __future__ import annotations

import json
import platform
import shlex
import shutil
import sys
from dataclasses import dataclass

from .. import bench
from . import gate, source, vm

# In the machine: the build directory the gate keeps warm between runs.
TARGET = "$HOME/targets/b3d-gate"
PROGRAM = "b3d-frame-counts"


@dataclass(frozen=True)
class Counted:
    """A board's instruction counts: each frame's, by frame index, and their sum."""

    frames: dict[int, int]

    @property
    def total(self) -> int:
        return sum(self.frames.values())


def parse(stdout: str) -> dict[str, Counted]:
    """Each board's counts from the machine script's lines: `frames <board> <n>` for the frames
    the program played and `count <board> <dump> <instructions>` for each dump. A board whose
    dumps do not number exactly one for each frame it played is left out."""
    played: dict[str, int] = {}
    dumps: dict[str, dict[int, int]] = {}
    for line in stdout.splitlines():
        fields = line.split()
        if len(fields) == 3 and fields[0] == "frames":
            played[fields[1]] = int(fields[2])
        elif len(fields) == 4 and fields[0] == "count":
            dumps.setdefault(fields[1], {})[int(fields[2])] = int(fields[3])
    found: dict[str, Counted] = {}
    for board, frames in played.items():
        counted = dumps.get(board, {})
        if sorted(counted) != list(range(1, frames + 1)):
            bench.log(
                f"{board}: played {frames} frames but Callgrind wrote {len(counted)} dumps"
            )
            continue
        found[board] = Counted({dump - 1: n for dump, n in counted.items()})
    return found


def worst(counts: Counted) -> tuple[int, int]:
    """The frame with the most instructions and its count, the earliest of equals."""
    frame = max(counts.frames, key=lambda f: (counts.frames[f], -f))
    return frame, counts.frames[frame]


def judge(
    name: str,
    counts: Counted,
    budgets: dict[str, gate.Budget],
    ranked: tuple[int, ...],
) -> tuple[list[str], list[str]]:
    """The failures and report lines for one board, with the frames ranked by native counters."""
    failed: list[str] = []
    said: list[str] = []
    frame, most = worst(counts)
    for key, value, where in (
        ("worst_frame_instructions", most, f"frame {frame}"),
        ("total_instructions", counts.total, "in total"),
    ):
        budget = budgets.get(key)
        line = f"{name} {where}: {key} is {value:,}"
        if budget is None:
            failed.append(f"{line} with no budget")
        elif value > budget["max"]:
            failed.append(f"{line}, over its budget {budget['max']:,}")
        else:
            said.append(f"{line}, budget {budget['max']:,}")
    if ranked and frame not in ranked:
        failed.append(
            f"{name}: frame {frame} has the most instructions ({most:,}) but the work"
            f" counters did not rank it among {sorted(ranked)}; make the counters see the"
            " work it does"
        )
    return failed, said


def scaling_failures(
    found: dict[str, Counted], scaling: dict[str, gate.Scaling]
) -> tuple[list[str], list[str]]:
    """The failures and the report lines of the scaling check: for each board under `scaling`,
    the ratio of its instruction counts to those of the board it names as `over` must fit
    the budget for each count. A pair with a board that was not counted is skipped.
    """
    failed: list[str] = []
    said: list[str] = []
    for name, bounds in sorted(scaling.items()):
        over = bounds["over"]
        large, small = found.get(name), found.get(over)
        if large is None or small is None:
            continue
        for key, value, base in (
            ("worst_frame_instructions", worst(large)[1], worst(small)[1]),
            ("total_instructions", large.total, small.total),
        ):
            budget = bounds["limits"].get(key)
            if budget is None:
                continue
            ratio = value / base
            line = (
                f"{name} over {over}: {key} {value:,} / {base:,} is {ratio:.3f} times,"
                f" against the budget {budget['max']}"
            )
            if ratio > budget["max"]:
                failed.append(
                    f"{line}; the cost grows faster than the board, so look for work done"
                    " for every pair of cards"
                )
            else:
                said.append(line)
    return failed, said


def script(boards: list[str]) -> str:
    """Provisions the machine, takes in the sources (`source.take_in_script`), builds
    `b3d-frame-counts`, and runs it for every board in `boards` at once under Callgrind,
    printing the lines `parse` reads."""
    build = [
        "cargo",
        "build",
        "--locked",
        "--release",
        "-q",
        "-p",
        "beryl_inspect",
        "--bin",
        PROGRAM,
    ]
    # The program turns collection on and off around each frame and dumps after it itself,
    # through client requests (`b3d-frame-counts`), so collection starts off.
    callgrind = [
        "valgrind",
        "--tool=callgrind",
        "--collect-atstart=no",
        "--dump-line=no",
        "--compress-strings=no",
    ]
    names = shlex.join(boards)
    work = f"""{source.take_in_script()}cd {source.SOURCE}/rust/beryl3d
{vm.JOBS_SCRIPT}CARGO_TARGET_DIR={TARGET} {shlex.join(build)}
out=$(mktemp -d)
pids=()
for board in {names}; do
  BERYL_CACHE="$HOME/{source.VM_CACHE}" {shlex.join(callgrind)} \\
    --callgrind-out-file="$out/$board.out" {TARGET}/release/{PROGRAM} "$board" \\
    >"$out/$board.frames" 2>"$out/$board.log" &
  pids+=($!)
done
# Each run's own exit status: `wait` on a remembered process ID returns it even after the
# process ended, where `$(jobs -p)` may no longer list it.
failed=0
for pid in "${{pids[@]}}"; do wait "$pid" || failed=1; done
for board in {names}; do
  if [ ! -s "$out/$board.frames" ]; then cat "$out/$board.log" >&2; failed=1; continue; fi
  echo "frames $board $(cat "$out/$board.frames")"
  for dump in "$out/$board.out".*; do
    echo "count $board ${{dump##*.}} $(sed -n 's/^totals: *//p' "$dump")"
  done
done
rm -rf "$out"
# The script's status is its last command's. Not `exit`: in this login shell `exit` runs
# Ubuntu's `~/.bash_logout`, whose `clear_console` fails with no terminal and replaces it.
test "$failed" = 0
"""
    return vm.provision_script(bench.toolchain()) + bench.locked(work)


def measure(boards: list[str]) -> str | None:
    """The machine script's output for `boards`, or `None` when the machine could not count."""
    if platform.machine() not in ("arm64", "aarch64"):
        bench.log("the frame counts are for Apple silicon's arm64; this machine is not")
        return None
    if shutil.which("limactl") is None:
        bench.log("Lima is missing: brew install lima, then `just b3d vm` once")
        return None
    if vm.start() != 0:
        return None
    if not source.send_pack():
        bench.log("the recordings pack could not be built or sent into the machine")
        return None
    run = source.run_with_sources(script(boards))
    if run is None:
        return None
    if run.returncode != 0:
        # What the script printed before it failed, which says how far it got.
        sys.stderr.write(run.stdout)
        bench.log(f"the count script failed with exit code {run.returncode}")
        return None
    return run.stdout


def run(boards: list[gate.Board], budgets: gate.Budgets, *, json_output: bool) -> int:
    stdout = measure([b.name for b in boards])
    if stdout is None:
        return 2
    found = parse(stdout)
    results = []
    measurements = {}
    for board in boards:
        counts = found.get(board.name)
        if counts is None:
            bench.log(f"Callgrind reported no counts for {board.name}")
            return 2
        results.append(
            judge(
                board.name, counts, budgets["boards"].get(board.name, {}), board.ranked
            )
        )
        measurements[board.name] = {
            "worst_frame_instructions": worst(counts)[1],
            "total_instructions": counts.total,
        }
    results.append(scaling_failures(found, budgets["scaling"]))
    if json_output:
        print(json.dumps({"boards": measurements}))
    failed = False
    for failures, said in results:
        if not json_output:
            for line in said:
                print(line)
        for line in failures:
            print(f"FAIL {line}", file=sys.stderr)
        failed |= bool(failures)
    return 1 if failed else 0
