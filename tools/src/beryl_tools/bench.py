"""Benchmarks (docs/benchmarking.md).

- `bench` builds the wall-clock benchmarks, refuses to time them on battery, in Low Power Mode
  or throttled, then runs them on this Mac. It times under whatever load the machine has, which
  is normally heavy (docs/benchmarking.md, "Timing under load"), and reports that load before
  and after. It builds them under the `search` profile for this CPU (architecture §18) unless
  the arguments name a `--profile`.
- `bench-count` runs the Gungraun (Valgrind) instruction-count benchmarks in a Lima Linux VM,
  creating and provisioning it on first use. Counts don't depend on load, so there's no check.
  Each worktree builds into a target dir of its own there: Cargo names a path package's
  artifacts the same in every worktree and trusts mtimes, so a shared dir would let one
  worktree link another's code. Runs take turns under a lock in the VM.
- `count-gate`, slow CI's `counts` lane, counts one benchmark there and fails when it rises past
  the bar in `beryl_bench/count-gate.toml`.

Extra arguments go to `cargo bench`: `by bench -p beryl_bench --bench example_abba`.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

from . import paths, system, warm

if TYPE_CHECKING:
    from .checks import Lane

# Compilers and the like, named in the load report: their bursts come and go faster than one
# CPU sample.
BUILD_PROCESSES = ("rustc", "cargo", "clippy-driver", "clang", "swift-frontend")

VM = "beryl-bench"
VM_LOCK = "$HOME/bench-count.lock"  # held while a run provisions, builds and counts
# Inside the VM, which mounts the checkout read-only, each worktree has a target dir and a
# Gungraun home named after it, so its artifacts and "old" results are its own.
VM_TARGETS = "$HOME/targets"
VM_GUNGRAUN = "$HOME/gungraun"
# In a target dir, per profile: the checkout that last built it. Another checkout's sources may
# be older than its artifacts, which Cargo would call fresh, so a new owner rebuilds the
# workspace members.
VM_OWNER = ".beryl-checkout"
COUNT_BENCHES = ["--bench", "*_counts"]
# A Cargo profile can't set the CPU, so the recipes building the `search` profile pass it: off
# Apple silicon, this machine's own (the justfile's `search_cpu`).
SEARCH_CPU = (
    "-C target-cpu=apple-m4"
    if platform.machine() in ("arm64", "aarch64")
    else "-C target-cpu=native"
)


def log(message: str) -> None:
    print(f"bench: {message}", file=sys.stderr)


def output(*cmd: str) -> str:
    run = subprocess.run(cmd, capture_output=True, text=True, check=False)
    return run.stdout


# What would skew the timing? Parsing is separate from probing so tests can feed it text.


@dataclass
class Machine:
    busy_percent: float
    on_battery: bool
    low_power: bool
    throttled: list[str] = field(default_factory=list)
    builds: list[str] = field(default_factory=list)
    busiest: list[str] = field(default_factory=list)


def parse_busy(top: str) -> float:
    """CPU in use from `top -l 2`: the last sample, since the first covers all of uptime."""
    samples = re.findall(r"CPU usage:.*?([\d.]+)% idle", top)
    return 100.0 - float(samples[-1]) if samples else 100.0


def parse_battery(pmset_batt: str) -> bool:
    return "'Battery Power'" in pmset_batt


def parse_low_power(pmset: str) -> bool:
    """Low Power Mode: `powermode 1` (0 automatic, 2 high) or the older `lowpowermode 1`."""
    return bool(re.search(r"^\s*(lowpowermode|powermode)\s+1\b", pmset, re.MULTILINE))


def parse_throttled(therm: str) -> list[str]:
    """Recorded thermal or performance warnings, and CPU speed limits below 100%."""
    found = []
    for line in therm.splitlines():
        level = re.search(r"(\w+) warning level\s*=\s*(\d+)", line, re.IGNORECASE)
        if level and level.group(2) != "0":
            found.append(line.strip())
        limit = re.search(r"CPU_Speed_Limit\s*=\s*(\d+)", line)
        if limit and int(limit.group(1)) < 100:
            found.append(line.strip())
    return found


def load(m: Machine) -> str:
    """The load timing runs under, for the log. It is never a reason to refuse: this machine
    always runs other sessions' builds and checks, and the ABBA harness's interval already
    widens with the noise they add (docs/benchmarking.md, "Timing under load")."""
    said = f"{m.busy_percent:.0f}% of the CPU in use"
    if m.busiest:
        said += "; busiest: " + "; ".join(m.busiest)
    if m.builds:
        said += f"; builds running: {', '.join(m.builds)}"
    return said


def problems(m: Machine) -> list[str]:
    """What slows the CPU for the whole run rather than adding noise to it, so a timing taken
    then says nothing about the machine as it normally runs."""
    found = []
    if m.on_battery:
        found.append("on battery: plug in")
    if m.low_power:
        found.append("Low Power Mode is on")
    found += [f"throttled: {t}" for t in m.throttled]
    return found


def probe_windows() -> Machine:
    """Windows has no top, pmset or pgrep: CPU in use over a second, the power line from
    GetSystemPowerStatus, and builds from tasklist. It reads no throttling."""
    import ctypes
    import time

    system.loadavg()
    time.sleep(1)
    busy = 100.0 * system.loadavg()[0] / (os.cpu_count() or 1)

    class PowerStatus(ctypes.Structure):
        _fields_ = [
            ("ac_line", ctypes.c_ubyte),
            ("battery_flag", ctypes.c_ubyte),
            ("battery_percent", ctypes.c_ubyte),
            ("saver", ctypes.c_ubyte),
            ("life", ctypes.c_ulong),
            ("full_life", ctypes.c_ulong),
        ]

    power = PowerStatus()
    ctypes.windll.kernel32.GetSystemPowerStatus(  # pyre-ignore[16]: Windows only
        ctypes.byref(power)
    )
    tasks = output("tasklist", "/fo", "csv", "/nh").splitlines()
    names = {row.split(",")[0].strip('"').removesuffix(".exe") for row in tasks}
    return Machine(
        busy_percent=busy,
        on_battery=power.ac_line == 0,
        low_power=power.saver == 1,
        builds=sorted(names & set(BUILD_PROCESSES)),
    )


def probe() -> Machine:
    if system.WINDOWS:
        return probe_windows()
    busiest = output("ps", "-Aco", "pcpu,comm", "-r").splitlines()[1:6]
    builds = output("pgrep", "-lx", "|".join(BUILD_PROCESSES)).split()
    return Machine(
        busy_percent=parse_busy(output("top", "-l", "2", "-n", "0", "-s", "1")),
        on_battery=parse_battery(output("pmset", "-g", "batt")),
        low_power=parse_low_power(output("pmset", "-g")),
        throttled=parse_throttled(output("pmset", "-g", "therm")),
        builds=sorted({b for b in builds if not b.isdigit()}),
        busiest=[" ".join(row.split()) for row in busiest],
    )


def cargo_bench(extra: list[str], *flags: str) -> list[str]:
    """`cargo bench` with our flags first: the user's may end in `-- <harness arguments>`."""
    return ["cargo", "bench", "--locked", *flags, *extra]


def named_profile(cargo: list[str]) -> str | None:
    """The profile the cargo flags name, ignoring harness arguments after `--`."""
    flags = cargo[: cargo.index("--")] if "--" in cargo else cargo
    for i, a in enumerate(flags):
        if a.startswith("--profile="):
            return a.removeprefix("--profile=")
        if a == "--profile" and i + 1 < len(flags):
            return flags[i + 1]
    return None


def timing_build(extra: list[str]) -> tuple[list[str], dict[str, str]]:
    """The `search` profile and its CPU, unless the cargo flags name a profile of their own."""
    if named_profile(extra) is not None:
        return [], {}
    return ["--profile", "search"], {"RUSTFLAGS": SEARCH_CPU}


def bench(args: argparse.Namespace) -> int:
    warm.ensure()
    extra: list[str] = args.extra
    profile, env = timing_build(extra)
    env = {**os.environ, **env}
    build = subprocess.run(
        cargo_bench(extra, "--no-run", *profile), cwd=paths.RUST, env=env, check=False
    )
    if build.returncode != 0:
        return build.returncode
    before = probe()
    found = problems(before)
    for p in found:
        log(p)
    if found and not args.force:
        log("not timing with the CPU slowed; fix the above, or pass --force")
        return 2
    log(f"timing under load: {load(before)}")
    run = cargo_bench(extra, *profile)
    code = subprocess.run(run, cwd=paths.RUST, env=env, check=False).returncode
    log(f"load after timing: {load(probe())}")
    return code


# The Lima VM for Gungraun; on Windows, a WSL distribution of the same name.


def toolchain() -> str:
    data = tomllib.loads((paths.REPO / "rust-toolchain.toml").read_text())
    return str(data["toolchain"]["channel"])


def locked_version(lock: str, package: str) -> str | None:
    match = re.search(
        rf'^name = "{re.escape(package)}"\nversion = "([^"]+)"', lock, re.MULTILINE
    )
    return match.group(1) if match else None


def provision_script(channel: str, runner: str) -> str:
    """Idempotent: installs Valgrind, the pinned Rust and the runner matching Cargo.lock."""
    return f"""set -euo pipefail
if ! command -v valgrind >/dev/null || ! command -v cc >/dev/null; then
  sudo apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq valgrind build-essential
fi
if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \\
    | sh -s -- -y --quiet --profile minimal --default-toolchain none
fi
. "$HOME/.cargo/env"
rustup toolchain install {channel} --profile minimal >/dev/null 2>&1
if ! gungraun-runner --version 2>/dev/null | grep -qF {runner}; then
  cargo +{channel} install --quiet --locked gungraun-runner --version {runner}
fi
"""


def wsl_status() -> str | None:
    """The WSL distribution's state, Running or Stopped; None if there is none."""
    run = subprocess.run(
        ["wsl", "--list", "--verbose"],
        capture_output=True,
        text=True,
        env={**os.environ, "WSL_UTF8": "1"},
        check=False,
    )
    for line in run.stdout.splitlines():
        fields = line.replace("*", " ").split()
        if len(fields) >= 2 and fields[0] == VM:
            return fields[1]
    return None


def vm_status() -> str | None:
    if system.WINDOWS:
        return wsl_status()
    for line in output(
        "limactl", "list", "--format", "{{.Name}} {{.Status}}"
    ).splitlines():
        name, _, status = line.partition(" ")
        if name == VM:
            return status
    return None


def vm_command(script: str) -> list[str]:
    if system.WINDOWS:
        # Windows' command line would mangle the script's quotes on the way through wsl.exe:
        # it goes as base64, and runs from a file so that its commands' stdin is not it.
        encoded = base64.b64encode(script.encode()).decode()
        unpack = f"f=$(mktemp); echo {encoded} | base64 -d >$f; bash -l $f; s=$?; rm -f $f; exit $s"
        return [
            "wsl",
            "-d",
            VM,
            "--cd",
            str(paths.RUST),
            "--exec",
            "bash",
            "-c",
            unpack,
        ]
    return ["limactl", "shell", "--workdir", str(paths.RUST), VM, "bash", "-lc", script]


def vm_shell(script: str) -> int:
    return subprocess.run(vm_command(script), check=False).returncode


def locked(script: str) -> str:
    """`script` holding the VM's lock, which the kernel drops when the shell exits, however."""
    return f"""exec 9>"{VM_LOCK}"
if ! flock -n 9; then
  echo "bench: another bench-count run is using the VM; waiting for it" >&2
  flock 9
fi
{script}"""


def count_script(
    provision: str, cargo: list[str], name: str, checkout: str, live: list[str]
) -> str:
    """Counts in `name`'s target dir and Gungraun home, as `checkout`'s build. A new target
    dir starts as a copy of the gate's warm one; those of worktrees not in `live` go."""
    target = f"{VM_TARGETS}/{shlex.quote(name)}"
    home = f"{VM_GUNGRAUN}/{shlex.quote(name)}"
    keep = " ".join(shlex.quote(n) for n in sorted({*live, name, GATE_NAME}))
    owner = shlex.quote(checkout)
    profile = shlex.quote(named_profile(cargo) or "bench")
    stamp = f"{target}/{VM_OWNER}.{profile}"
    prepare = f"""shopt -s nullglob
keep=({keep})
for dir in {VM_TARGETS}/* {VM_GUNGRAUN}/*; do
  printf '%s\\n' "${{keep[@]}}" | grep -qxF -- "${{dir##*/}}" || rm -rf -- "$dir"
done
if [ ! -d {target} ] && [ -d {VM_TARGETS}/{GATE_NAME} ]; then
  cp -a {VM_TARGETS}/{GATE_NAME} {target}
fi
if [ "$(cat {stamp} 2>/dev/null)" != {owner} ]; then
  cargo clean --quiet --workspace --profile {profile} --target-dir {target}
  mkdir -p {target} && printf '%s\\n' {owner} >{stamp}
fi
"""
    env = f"CARGO_TARGET_DIR={target} GUNGRAUN_HOME={home}"
    return locked(f"{provision}{prepare}{env} {shlex.join(cargo)}\n")


def wsl_ensure() -> int:
    """Creates the WSL distribution, its disk in the cache (on the Dev Drive, if the cache is),
    where it runs as root; WSL starts it for each command."""
    if shutil.which("wsl") is None:
        log("WSL is missing: wsl --install, in an administrator's terminal")
        return 2
    if wsl_status() is not None:
        return 0
    log(f"creating the {VM} WSL distribution (downloads Ubuntu once; a few minutes)")
    return subprocess.run(
        ["wsl", "--install", "Ubuntu-24.04", "--name", VM, "--no-launch"]
        + ["--location", str(paths.cache_root() / "wsl" / VM)],
        check=False,
    ).returncode


def vm_ensure() -> int:
    if system.WINDOWS:
        return wsl_ensure()
    if not output("which", "limactl").strip():
        log("Lima is missing: brew install lima")
        return 2
    status = vm_status()
    checkout = paths.main_checkout() or paths.REPO
    if status is None:
        log(f"creating the {VM} VM (downloads Ubuntu once; a few minutes)")
        cmd = ["limactl", "start", f"--name={VM}", "--tty=false", "--cpus=4"]
        cmd += ["--memory=4", "--containerd=none", f"--mount-only={checkout}"]
        code = subprocess.run([*cmd, "template:ubuntu-lts"], check=False).returncode
    elif status != "Running":
        code = subprocess.run(["limactl", "start", "--tty=false", VM]).returncode
    else:
        code = 0
    return code


def bench_count(args: argparse.Namespace) -> int:
    code = vm_ensure()
    if code != 0:
        return code
    runner = locked_version((paths.RUST / "Cargo.lock").read_text(), "gungraun")
    if runner is None:
        log("no gungraun in rust/Cargo.lock")
        return 2
    extra: list[str] = args.extra or COUNT_BENCHES
    script = count_script(
        provision_script(toolchain(), runner),
        cargo_bench(extra),
        paths.REPO.name,
        str(paths.REPO),
        paths.worktree_names(),
    )
    return vm_shell(script)


# The instruction-count gate: slow CI's `counts` lane. Its bar, and when to move it, are in
# GATE_FILE's comments.

GATE_FILE: Path = paths.RUST / "crates" / "beryl_bench" / "count-gate.toml"
# Its own target dir and Gungraun home, shared by CI's throwaway worktrees so the build stays
# warm and they don't each leave one in the VM.
GATE_NAME = "count-gate"


@dataclass
class Gate:
    benchmark: str  # FILE::GROUP::FUNCTION::ID, as Gungraun filters and reports it
    instructions: int
    tolerance_percent: float


def load_gate(text: str) -> Gate:
    data = tomllib.loads(text)
    return Gate(
        benchmark=str(data["benchmark"]),
        instructions=int(data["instructions"]),
        tolerance_percent=float(data["tolerance_percent"]),
    )


def gate_cargo(gate: Gate) -> list[str]:
    """Builds and counts only the gated benchmark, printing Gungraun's JSON summary."""
    file = gate.benchmark.split("::")[0]
    return cargo_bench(["--bench", file, "--", "--output-format=json", gate.benchmark])


def counted(stdout: str, benchmark: str) -> int | None:
    """The benchmark's instructions (Callgrind's `Ir`) from Gungraun's JSON lines."""
    for line in stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            summary = json.loads(line)
        except json.JSONDecodeError:
            continue
        if f"{summary.get('module_path')}::{summary.get('id')}" != benchmark:
            continue
        for profile in summary.get("profiles", []):
            if profile.get("tool") == "Callgrind":
                return int(profile["data"]["total"]["metrics"]["Ir"]["values"]["new"])
    return None


def judge(gate: Gate, measured: int) -> tuple[bool, str]:
    """Fails a rise past the tolerance; a drop past it passes, asking for a lower bar."""
    change = 100.0 * (measured - gate.instructions) / gate.instructions
    said = f"{measured:,} instructions, {change:+.2f}% against the bar of {gate.instructions:,}"
    if change > gate.tolerance_percent:
        return False, (
            f"{said}: over the {gate.tolerance_percent}% tolerance. Make the new cost free "
            f"for these decks, or raise the bar only as {GATE_FILE.name} allows"
        )
    if change < -gate.tolerance_percent:
        return (
            True,
            f"{said}: lower `instructions` in {GATE_FILE.name} to keep the gain",
        )
    return True, said


def lane() -> Lane:
    """Counts the rollout benchmark's instructions in the bench VM (`by count-gate`) and
    fails a rise past the bar in `beryl_bench/count-gate.toml`. The VM builds into its own
    target dir, so it needs no warm clone."""
    from .checks import BY, Lane  # It imports this module.
    from .reruns import whole

    return Lane("counts").step("count-gate", [*BY, "count-gate"], rerun=whole)


def count_gate(args: argparse.Namespace) -> int:
    if platform.machine() not in ("arm64", "aarch64"):
        # The bar counts an aarch64 build's instructions; an x86-64 build runs others.
        log("the gate's bar is for Apple silicon; `just bench-count` compares here")
        return 2
    if not system.WINDOWS and shutil.which("limactl") is None:
        log("Lima is missing: brew install lima, then `just bench-count` once")
        return 2
    if vm_status() is None:
        # Creating it takes minutes, past CI's deadline.
        log(f"no {VM} VM: run `just bench-count` once to create it")
        return 2
    code = vm_ensure()
    if code != 0:
        return code
    runner = locked_version((paths.RUST / "Cargo.lock").read_text(), "gungraun")
    if runner is None:
        log("no gungraun in rust/Cargo.lock")
        return 2
    gate = load_gate(GATE_FILE.read_text())
    script = count_script(
        provision_script(toolchain(), runner),
        gate_cargo(gate),
        GATE_NAME,
        str(paths.REPO),
        paths.worktree_names(),
    )
    run = subprocess.run(
        vm_command(script),
        stdout=subprocess.PIPE,
        text=True,
        check=False,
    )
    if run.returncode != 0:
        return run.returncode
    measured = counted(run.stdout, gate.benchmark)
    if measured is None:
        log(f"Gungraun reported no count for {gate.benchmark}")
        return 2
    ok, message = judge(gate, measured)
    log(f"{gate.benchmark}: {message}")
    return 0 if ok else 1


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "bench",
        help="time benchmarks on this Mac, under its usual load (cargo bench flags)",
        allow_abbrev=False,
    )
    p.add_argument(
        "--force",
        action="store_true",
        help="time even on battery, in Low Power Mode or throttled",
    )
    p.set_defaults(func=bench, passthrough=True)
    p = sub.add_parser(
        "bench-count",
        help="Gungraun instruction counts in a Lima VM, or WSL (cargo bench flags)",
        allow_abbrev=False,
    )
    p.set_defaults(func=bench_count, passthrough=True)
    p = sub.add_parser(
        "count-gate",
        help="fail when the gated benchmark's instruction count rises past its bar",
    )
    p.set_defaults(func=count_gate)


if __name__ == "__main__":
    sys.exit(f"run it as `by bench`, not {os.path.basename(sys.argv[0])}")
