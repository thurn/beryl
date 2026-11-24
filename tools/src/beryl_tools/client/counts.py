"""`just client gate --counts` counts every frame under Callgrind in `beryl3d-bench`.
For each board it checks the worst frame (its `handle` calls and its `frame` call) and the
total instructions against `data/client/budgets.json`.

The scaling check compares `stress-1024` with `stress-255`. Both boards play the same table,
with four times as many cards on the larger board. Its counts can grow by at most the ratio
measured when the work was linear, plus headroom.

The frame with the most instructions must be among the frames `rank` picks from the native work
counters, so the native gate keeps seeing the work that costs instructions.

The Linux guest gets a copy of the worktree (`source.py`) and the native recordings pack.
"""

from __future__ import annotations

import json
import os
import shlex
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from .. import bench, calibration, system, vm_host
from . import frame_profiles, gate, source, vm

# In the machine: the build directory the gate keeps warm between runs.
TARGET = "$HOME/targets/client-gate"
PROGRAM = "client-frame-counts"


def runtime_script(channel: str) -> str:
    """Print the tool versions from the same guest invocation that counts the frames."""
    backend = "wsl" if system.WINDOWS else "lima"
    return f""". /etc/os-release
printf 'runtime backend={backend}\\n'
printf 'runtime guest=%s %s\\n' "$ID" "$VERSION_ID"
printf 'runtime arch=%s\\n' "$(uname -m)"
printf 'runtime valgrind=%s\\n' "$(valgrind --version)"
printf 'runtime libc=%s\\n' "$(getconf GNU_LIBC_VERSION)"
printf 'runtime cc=%s\\n' "$(cc -dumpfullversion)"
printf 'runtime rustc=%s\\n' "$(rustc +{shlex.quote(channel)} --version)"
"""


def runtime(stdout: str) -> dict[str, str]:
    found: dict[str, str] = {}
    for line in stdout.splitlines():
        if line.startswith("runtime "):
            key, separator, value = line.removeprefix("runtime ").partition("=")
            if separator:
                found[key] = value
    return found


def live_profile() -> calibration.Profile:
    """Validate the listing digest with the live tools before Ignite can reuse a verdict."""
    profile = frame_profiles.registered("instructions")
    launcher = "wsl" if system.WINDOWS else "limactl"
    if shutil.which(launcher) is None or vm_host.status(vm.NAME) is None:
        raise calibration.SetupError(f"set up {vm.NAME} with `just client vm`")
    if vm.start() != 0:
        raise calibration.SetupError(f"could not start {vm.NAME}")
    channel = bench.toolchain()
    script = f'set -e\n. "$HOME/.cargo/env"\n{runtime_script(channel)}'
    probe = subprocess.run(
        bench.vm_command(script, vm.NAME, "/"),
        cwd=vm_host.shell_host_cwd(),
        capture_output=True,
        text=True,
        check=False,
    )
    if probe.returncode:
        raise calibration.SetupError(
            f"could not identify tools in {vm.NAME}; run `just client vm`"
        )
    calibration.require_runtime(profile, runtime(probe.stdout))
    return profile


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
        if frames <= 0 or sorted(counted) != list(range(1, frames + 1)):
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


def script(boards: list[str], *, wait: bool = True) -> str:
    """Provisions the machine, takes in the sources (`source.take_in_script`), builds
    `client-frame-counts`, and runs it for every board in `boards` under Callgrind, within
    Ignite's CPU grant. It prints the lines `parse` reads."""
    channel = bench.toolchain()
    build = [
        "cargo",
        f"+{channel}",
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
    # through client requests (`client-frame-counts`), so collection starts off.
    callgrind = [
        "valgrind",
        "--tool=callgrind",
        "--collect-atstart=no",
        "--dump-line=no",
        "--compress-strings=no",
    ]
    names = shlex.join(boards)
    grant = max(1, int(os.environ.get("IGNITE_CORES") or vm.CPUS))
    work = f"""{runtime_script(channel)}{source.take_in_script()}cd {source.SOURCE}/client
{vm.JOBS_SCRIPT}if ((CARGO_BUILD_JOBS > {grant})); then export CARGO_BUILD_JOBS={grant}; fi
CARGO_TARGET_DIR={TARGET} {shlex.join(build)}
# The dumps are one file per frame, over a gigabyte for a run. They go on the machine's disk,
# not `/tmp`, which is a 6 GiB tmpfs: a run that was killed before it removed its directory
# left it there, and after a few of them Callgrind could not even create its `vgdb` pipe, so
# every later run failed with no message of its own. This script holds the machine's lock,
# so any directory a former run left is stale.
rm -rf "$HOME"/client-counts.*
out=$(mktemp -d "$HOME/client-counts.XXXXXX")
trap 'rm -rf "$out"' EXIT
pids=()
# A completed process stays waitable by ID. Keep every status, even for boards that
# finished before the next one starts.
failed=0
for board in {names}; do
  BERYL_CACHE="$HOME/{source.VM_CACHE}" {shlex.join(callgrind)} \\
    --callgrind-out-file="$out/$board.out" {TARGET}/release/{PROGRAM} "$board" \\
    >"$out/$board.frames" 2>"$out/$board.log" &
  pids+=($!)
  if ((${{#pids[@]}} >= {grant})); then
    wait "${{pids[0]}}" || failed=1
    pids=("${{pids[@]:1}}")
  fi
done
for pid in "${{pids[@]}}"; do wait "$pid" || failed=1; done
for board in {names}; do
  if [ ! -s "$out/$board.frames" ]; then cat "$out/$board.log" >&2; failed=1; continue; fi
  echo "frames $board $(cat "$out/$board.frames")"
  for dump in "$out/$board.out".*; do
    echo "count $board ${{dump##*.}} $(sed -n 's/^totals: *//p' "$dump")"
  done
done
# The script's status is its last command's. Not `exit`: in this login shell `exit` runs
# Ubuntu's `~/.bash_logout`, whose `clear_console` fails with no terminal and replaces it.
test "$failed" = 0
"""
    provision = vm.provision_script(channel)
    if wait:
        return bench.locked(provision + work)
    return bench.locked(provision + work, wait=False)


def measure(
    boards: list[str], *, wait: bool = True, pack: Path | None = None
) -> str | None:
    """The machine script's output for `boards`, or `None` when the machine could not count."""
    launcher = "wsl" if system.WINDOWS else "limactl"
    if shutil.which(launcher) is None:
        bench.log(f"{launcher} is missing; install it and run `just client vm`")
        return None
    if not wait and vm_host.status(vm.NAME) is None:
        bench.log(f"{vm.NAME} is not configured; skipping optional counts")
        return None
    if vm.start() != 0:
        return None
    # Avoid sending the pack when the VM is busy. The count script tries the same
    # locks again before changing the VM's sources in case another job took a turn.
    if not wait and bench.vm_shell(bench.locked("true", wait=False), vm.NAME, "/") != 0:
        return None
    if not source.send_pack(pack):
        bench.log("the recordings pack could not be built or sent into the machine")
        return None
    run = source.run_with_sources(script(boards, wait=wait))
    if run is None:
        return None
    if run.returncode != 0:
        # What the script printed before it failed, which says how far it got.
        sys.stderr.write(run.stdout)
        bench.log(f"the count script failed with exit code {run.returncode}")
        return None
    return run.stdout


def counters(counts: Counted) -> dict[str, float]:
    return {
        "worst_frame_instructions": worst(counts)[1],
        "total_instructions": counts.total,
    }


def verdict(
    boards: list[gate.Board], found: dict[str, Counted], budgets: gate.Budgets
) -> tuple[list[str], list[str]]:
    failed, said = scaling_failures(found, budgets["scaling"])
    for board in boards:
        failures, lines = judge(
            board.name,
            found[board.name],
            budgets["boards"].get(board.name, {}),
            board.ranked,
        )
        failed.extend(failures)
        said.extend(lines)
    return failed, said


def budgets_for(budgets: gate.Budgets, profile: calibration.Profile) -> gate.Budgets:
    """The verifier must supply the whole set of instruction limits."""
    boards = profile.limits.get("boards")
    scaling = profile.limits.get("scaling")
    metrics = {"worst_frame_instructions", "total_instructions"}
    if not isinstance(boards, dict):
        raise calibration.SetupError(
            f"{profile.verifier} has no per-board instruction limits"
        )
    for board, limits in budgets["boards"].items():
        overrides = boards.get(board, {})
        if metrics & limits.keys() and (
            not isinstance(overrides, dict) or not metrics <= overrides.keys()
        ):
            raise calibration.SetupError(
                f"{profile.verifier} has no instruction limits for {board}"
            )
    if not isinstance(scaling, dict) or scaling.keys() != budgets["scaling"].keys():
        raise calibration.SetupError(
            f"{profile.verifier} has incomplete frame scaling limits"
        )
    for name, reference in budgets["scaling"].items():
        override = scaling[name]
        if (
            not isinstance(override, dict)
            or override.get("over") != reference["over"]
            or not metrics <= override.get("limits", {}).keys()
        ):
            raise calibration.SetupError(
                f"{profile.verifier} has no scaling limits for {name} over {reference['over']}"
            )
    return gate.profile_budgets(budgets, profile)


def collect(
    boards: list[gate.Board],
    profile: calibration.Profile,
    *,
    wait: bool = True,
    pack: Path | None = None,
) -> dict[str, Counted]:
    output = measure([board.name for board in boards], wait=wait, pack=pack)
    if output is None:
        raise calibration.SetupError("Callgrind could not measure the boards")
    calibration.require_runtime(profile, runtime(output))
    found = parse(output)
    for board in boards:
        counted = found.get(board.name)
        if counted is None or len(counted.frames) != len(board.frames):
            raise calibration.SetupError(
                f"Callgrind did not count every native frame of {board.name}"
            )
    return found


def run(
    boards: list[gate.Board],
    budgets: gate.Budgets,
    *,
    json_output: bool,
    wait: bool = True,
    pack: Path | None = None,
) -> int:
    try:
        profile = frame_profiles.registered("instructions")
        budgets = budgets_for(budgets, profile)
        found = collect(boards, profile, wait=wait, pack=pack)
    except calibration.SetupError as error:
        bench.log(str(error))
        return 2
    failures, said = verdict(boards, found, budgets)
    if json_output:
        measurements = {name: counters(board) for name, board in found.items()}
        print(
            json.dumps(
                {
                    "boards": measurements,
                    "verifier": profile.verifier,
                    "calibration": profile.digest,
                }
            )
        )
    else:
        for line in said:
            print(line)
    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if failures else 0
