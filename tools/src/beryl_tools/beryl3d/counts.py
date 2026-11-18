"""`just b3d gate --counts`: the instruction-count gate for frames (docs/beryl-3d.md, 'Finding
the worst frame'). It counts every frame of each gated board under Callgrind in
`beryl3d-bench` and holds each board to the limits in `data/beryl3d/gates.json` under
`frames.<board>`:

- `worst_frame_instructions`: the most instructions in one frame (its `handle` calls and its
  `frame` call).
- `total_instructions`: the sum over every frame of the playback.

Counts are exact, so the tolerance a count may pass its limit by is not for noise. After the
build-out it is 1%, which absorbs the small shifts when an unrelated change alters how the
compiler inlines and lays out this code, as the engine's count gate does, and a count further
below its limit fails too, until `--update` lowers the limit. During the build-out it is 20% and
a count below its limit passes (build_out.py).

The scaling check holds the ratio of one board's counts to a smaller board's under `scaling` in
`gates.json`: `stress-1024` plays the same table as `stress-255` with four times the cards, so
its counts may grow by at most the ratio measured once the work was linear, plus a margin. Work
that grows faster than the board, such as a search over every card for every moving card, fails
it however many verbs land beside it.

It also checks the native work counters: the frame with the most instructions must be among the
frames `rank` picks from them, so that the native gate, which runs in seconds, keeps seeing the
work that costs instructions.

Each board is one run of `b3d-frame-counts` under Callgrind, all boards at once. The program
asks Callgrind to collect only around each frame and to dump its counts after it, so dump `n`
holds frame `n - 1`. Before, Gungraun ran one Valgrind
process per counted frame, each replaying the board up to it: about 100 processes and 25
minutes for the four boards.

`--update` lowers each limit to what was measured and never raises one. A limit rises only by
hand, when the work itself changed or a compiler or dependency upgrade moved the count, with the
old and new numbers in the commit body (rust/beryl3d/AGENTS.md).

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
from typing import Any

from .. import bench
from .. import paths as repo_paths
from . import build_out, gate, source, vm

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
    limits: dict[str, Any],
    ranked: tuple[int, ...],
    *,
    tolerance: float,
    fail_below: bool,
) -> tuple[list[str], list[str]]:
    """The failures and the report lines for one board, whose work counters ranked the frames
    `ranked` highest. A count may pass its limit by `tolerance` percent, and when `fail_below`
    is set it fails further below its limit than that too."""
    failed: list[str] = []
    said: list[str] = []
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
        elif change < -tolerance and fail_below:
            # Room under a limit is room a later change could fill unseen, so a change that
            # cut the work lowers the limit in the same commit.
            failed.append(
                f"{line}, under it by more than the tolerance of {tolerance}%;"
                " `just b3d gate --counts --update` lowers it"
            )
        else:
            said.append(line)
    if ranked and frame not in ranked:
        failed.append(
            f"{name}: frame {frame} has the most instructions ({most:,}) but the work"
            f" counters did not rank it among {sorted(ranked)}; make the counters see the"
            " work it does"
        )
    return failed, said


def lowered(name: str, counts: Counted, limits: dict[str, Any]) -> dict[str, Any]:
    """`limits` with each instruction limit lowered to what was measured, or set from it when
    missing. No limit rises."""
    updated = dict(limits)
    _, most = worst(counts)
    for key, value in (
        ("worst_frame_instructions", most),
        ("total_instructions", counts.total),
    ):
        old = updated.get(key)
        updated[key] = value if old is None else min(int(old), value)
    # The tolerance is build_out.py's, the same for every board.
    updated.pop("tolerance_percent", None)
    return updated


def scaling_failures(
    found: dict[str, Counted], scaling: dict[str, dict[str, Any]]
) -> tuple[list[str], list[str]]:
    """The failures and the report lines of the scaling check: for each board under `scaling`,
    the ratio of its instruction counts to those of the board it names as `over` must be at most
    the bound given for each count. A pair with a board that was not counted is skipped.
    """
    failed: list[str] = []
    said: list[str] = []
    for name, bounds in sorted(scaling.items()):
        over = str(bounds["over"])
        large, small = found.get(name), found.get(over)
        if large is None or small is None:
            continue
        for key, value, base in (
            ("worst_frame_instructions", worst(large)[1], worst(small)[1]),
            ("total_instructions", large.total, small.total),
        ):
            bound = bounds.get(key)
            if bound is None:
                continue
            ratio = value / base
            line = (
                f"{name} over {over}: {key} {value:,} / {base:,} is {ratio:.3f} times,"
                f" against the bound {bound}"
            )
            if ratio > float(bound):
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


def run(boards: list[gate.Board], update: bool) -> int:
    stdout = measure([b.name for b in boards])
    if stdout is None:
        return 2
    found = parse(stdout)
    data = json.loads(gate.GATES.read_text())
    frames: dict[str, dict[str, Any]] = data.setdefault("frames", {})
    measured: list[tuple[gate.Board, Counted]] = []
    for board in boards:
        counts = found.get(board.name)
        if counts is None:
            bench.log(f"Callgrind reported no counts for {board.name}")
            return 2
        measured.append((board, counts))
    if update:
        for board, counts in measured:
            frames[board.name] = lowered(board.name, counts, frames.get(board.name, {}))
        gate.GATES.write_text(json.dumps(data, indent=2) + "\n")
        print(f"wrote {gate.GATES.relative_to(repo_paths.REPO)}")
    tolerance = build_out.tolerance_percent()
    results = [
        judge(
            board.name,
            counts,
            frames.get(board.name, {}),
            board.ranked,
            tolerance=tolerance,
            fail_below=not build_out.BUILD_OUT,
        )
        for board, counts in measured
    ]
    results.append(scaling_failures(found, data.get("scaling", {})))
    failed = False
    for failures, said in results:
        for line in said:
            print(line)
        for line in failures:
            print(f"FAIL {line}", file=sys.stderr)
        failed |= bool(failures)
    return 1 if failed else 0
