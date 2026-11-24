"""Benchmarks (docs/benchmarking.md).

- `bench` builds the wall-clock benchmarks, refuses to time them on battery, in Low Power Mode
  or throttled, then runs them on this Mac. It times under whatever load the machine has, which
  is normally heavy (docs/benchmarking.md, "Timing under load"), and reports that load before
  and after. It builds them under the `search` profile for this CPU (architecture §18) unless
  the arguments name a `--profile`.
- `bench-count` runs the Gungraun (Valgrind) instruction-count benchmarks in Linux under Lima or
  WSL, creating and provisioning it on first use. Counts don't depend on load.
  Each worktree builds into a target dir of its own there: Cargo names a path package's
  artifacts the same in every worktree and trusts mtimes, so a shared dir would let one
  worktree link another's code. Runs take turns under a lock in the VM.
- `count-gate` (`count_gate.py`) judges rollout and paired present-cost counts from that guest.

Extra arguments go to `cargo bench`: `by bench -p beryl_bench --bench example_abba`.
"""

from __future__ import annotations

import argparse
import base64
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

from . import paths, system, vm_host

# Compilers and the like, named in the load report: their bursts come and go faster than one
# CPU sample.
BUILD_PROCESSES = ("rustc", "cargo", "clippy-driver", "clang", "swift-frontend")

VM = "beryl-bench"
VM_LOCK = "$HOME/bench-count.lock"  # held while a run provisions, builds and counts
# Held shared by each run someone waits for, so that a low-priority run goes after it ([`locked`]).
VM_TURN = "$HOME/bench-turn.lock"
# Inside the VM, each worktree has a target dir and a
# Gungraun home named after it, so its artifacts and "old" results are its own.
VM_TARGETS = "$HOME/targets"
VM_GUNGRAUN = "$HOME/gungraun"
# In a target dir, per profile: the checkout that last built it. Another checkout's sources may
# be older than its artifacts, which Cargo would call fresh, so a new owner rebuilds the
# workspace members.
VM_OWNER = ".beryl-checkout"
# The gate shares a warm target among throwaway worktrees.
GATE_NAME = "count-gate"
COUNT_BENCHES = ["--bench", "*_counts"]
COUNT_SOURCES = (
    "engine",
    "rust-toolchain.toml",
    "data",
    "docs/sample-decks",
)
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


# The package benchmarks build as, unless the flags name one. Built from the workspace with no
# package named, Cargo turns on every feature any member's dev-dependencies ask for, which no
# build of the app or the search has. On 2026-11-13 one such feature, the card tests' record of
# which abilities ran (now `beryl_engine::coverage`, kept only with debug assertions), was 0.42%
# of the rollout benchmark's instructions.
BENCH_PACKAGE = "beryl_bench"


def cargo_bench(extra: list[str], *flags: str) -> list[str]:
    """`cargo bench` with our flags first: the user's may end in `-- <harness arguments>`.
    It builds [`BENCH_PACKAGE`] alone unless the user's flags pick the packages."""
    ours = cargo_flags(extra)
    picks = any(
        f in ("-p", "--package", "--workspace", "--all") or f.startswith("--package=")
        for f in ours
    )
    package = [] if picks else ["-p", BENCH_PACKAGE]
    return ["cargo", "bench", "--locked", *package, *flags, *extra]


def cargo_flags(cargo: list[str]) -> list[str]:
    """The cargo flags, leaving out harness arguments after `--`."""
    return cargo[: cargo.index("--")] if "--" in cargo else cargo


def named_profile(cargo: list[str]) -> str | None:
    """The profile the cargo flags name, ignoring harness arguments after `--`."""
    flags = cargo_flags(cargo)
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
    extra: list[str] = args.extra
    profile, env = timing_build(extra)
    env = {**os.environ, **env}
    build = subprocess.run(
        cargo_bench(extra, "--no-run", *profile), cwd=paths.ENGINE, env=env, check=False
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
    code = subprocess.run(run, cwd=paths.ENGINE, env=env, check=False).returncode
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


def provision_script(channel: str, runner: str | None) -> str:
    """Idempotent: installs Valgrind, the pinned Rust and, unless `runner` is `None`, the
    Gungraun runner of that release, which must match Cargo.lock's."""
    script = f"""set -euo pipefail
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
"""
    if runner is None:
        return script
    return (
        script
        + f"""if ! gungraun-runner --version 2>/dev/null | grep -qF {runner}; then
  cargo +{channel} install --quiet --locked gungraun-runner --version {runner}
fi
"""
    )


def vm_command(script: str, vm: str = VM, workdir: str | None = None) -> list[str]:
    """`script` run in `vm`'s login shell from `workdir`, by default this checkout's engine/
    directory. Scripts that send sources from an unmounted worktree start at `/`."""
    where = str(paths.ENGINE) if workdir is None else workdir
    if system.WINDOWS:
        # Windows' command line would mangle the script's quotes on the way through wsl.exe:
        # it goes as base64, and runs from a file so that its commands' stdin is not it.
        encoded = base64.b64encode(script.encode()).decode()
        unpack = f"f=$(mktemp); echo {encoded} | base64 -d >$f; bash -l $f; s=$?; rm -f $f; exit $s"
        return [
            "wsl",
            "-d",
            vm,
            "--cd",
            where,
            "--exec",
            "bash",
            "-c",
            unpack,
        ]
    return ["limactl", "shell", "--workdir", where, vm, "bash", "-lc", script]


def vm_shell(script: str, vm: str = VM, workdir: str | None = None) -> int:
    return subprocess.run(
        vm_command(script, vm, workdir), cwd=vm_host.shell_host_cwd(), check=False
    ).returncode


def locked(script: str, background: bool | None = None, *, wait: bool = True) -> str:
    """`script` holding the VM's lock, which the kernel drops when the shell exits, however. A
    run someone waits for holds `VM_TURN` shared while it waits and runs; a low-priority run
    (`background`, by default `vm_host.background`) takes `VM_LOCK` only while it can hold
    `VM_TURN` alone. `flock` hands a freed lock to any waiter, so a worker's `just client gate
    --gpu` once waited 66 minutes behind low-priority runs. A run that has started still goes
    first. With `wait=False`, try both locks once without taking a turn ahead of a waiter.
    """
    if not wait:
        return f"""exec 8>"{VM_TURN}"
exec 9>"{VM_LOCK}"
if flock -n -x 8 && flock -n 9; then
  flock -u 8
  {script}
else
  echo "bench: the VM is busy; skipping optional counts" >&2
  false
fi"""
    if background is None:
        background = vm_host.background()
    if not background:
        return f"""exec 8>"{VM_TURN}"
flock -s 8
exec 9>"{VM_LOCK}"
if ! flock -n 9; then
  echo "bench: another bench-count run is using the VM; waiting for it" >&2
  flock 9
fi
{script}"""
    return f"""exec 8>"{VM_TURN}"
exec 9>"{VM_LOCK}"
waited=""
while :; do
  flock -x 8
  if flock -n 9; then flock -u 8; break; fi
  flock -u 8
  [ -n "$waited" ] || echo "bench: the VM is busy or a waited-for run goes first; waiting" >&2
  waited=1
  sleep 5
done
{script}"""


def count_script(
    provision: str,
    cargo: list[str],
    name: str,
    checkout: str,
    live: list[str],
    then: list[list[str]] | None = None,
    source: str = "",
) -> str:
    """Counts in `name`'s target dir and Gungraun home, as `checkout`'s build, running each
    command of `then` after `cargo` if it succeeded. A new target dir copies the gate's warm
    one; those of worktrees not in `live` go. The gate's home is emptied."""
    target = f"{VM_TARGETS}/{shlex.quote(name)}"
    home = f"{VM_GUNGRAUN}/{shlex.quote(name)}"
    keep = " ".join(shlex.quote(n) for n in sorted({*live, name, GATE_NAME}))
    owner = shlex.quote(checkout)
    profile = shlex.quote(named_profile(cargo) or "bench")
    stamp = f"{target}/{VM_OWNER}.{profile}"
    fresh = f"rm -rf -- {home}\n" if name == GATE_NAME else ""
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
{fresh}"""
    env = f"CARGO_TARGET_DIR={target} GUNGRAUN_HOME={home}"
    commands = " && ".join(f"{env} {shlex.join(c)}" for c in [cargo, *(then or [])])
    # Parallel Rust compilers can exhaust the Lima guest's default 4 GiB on a cold build.
    jobs = "" if system.WINDOWS else "export CARGO_BUILD_JOBS=1\n"
    return locked(f"{source}{provision}{jobs}{prepare}{commands}\n")


def wsl_ensure(vm: str = VM) -> int:
    """Creates the WSL distribution, its disk in the cache (on the Dev Drive, if the cache is),
    where it runs as root; WSL starts it for each command."""
    if shutil.which("wsl") is None:
        log("WSL is missing: wsl --install, in an administrator's terminal")
        return 2
    if vm_host.wsl_status(vm) is not None:
        return 0
    log(f"creating the {vm} WSL distribution (downloads Ubuntu once; a few minutes)")
    return subprocess.run(
        ["wsl", "--install", "Ubuntu-24.04", "--name", vm, "--no-launch"]
        + ["--location", str(paths.cache_root() / "wsl" / vm)],
        check=False,
    ).returncode


def vm_ensure(vm: str = VM, cpus: int = 4, memory_gib: int = 4) -> int:
    """Creates the machine `vm` with `cpus` CPUs and `memory_gib` GiB, or starts it. A stopped
    machine smaller than that grows first. A running one is left as it is, since stopping it
    would kill another session's run, and grows the next time it is found stopped."""
    if system.WINDOWS:
        return wsl_ensure(vm)
    if not output("which", "limactl").strip():
        log("Lima is missing: brew install lima")
        return 2
    # This command uses the machine until it exits, so it is not stopped as idle meanwhile
    # (vm_host.py).
    vm_host.join(vm)
    status = vm_host.status(vm)
    checkout = paths.main_checkout() or paths.REPO
    size = [f"--cpus={cpus}", f"--memory={memory_gib}"]
    if status is None:
        log(f"creating the {vm} VM (downloads Ubuntu once; a few minutes)")
        cmd = ["limactl", "start", f"--name={vm}", "--tty=false", *size]
        cmd += ["--containerd=none", f"--mount-only={checkout}"]
        code = subprocess.run(
            [*cmd, "template:ubuntu-lts"], cwd=vm_host.LIMA_HOST_CWD, check=False
        ).returncode
    elif status != "Running":
        now = vm_host.size(vm)
        if now is not None and (now[0] < cpus or now[1] < memory_gib):
            log(f"growing the {vm} VM from {now[0]} CPUs and {now[1]} GiB")
            edit = ["limactl", "edit", "--tty=false", *size, vm]
            if subprocess.run(edit, check=False).returncode != 0:
                return 2
        code = subprocess.run(
            ["limactl", "start", "--tty=false", vm],
            cwd=vm_host.LIMA_HOST_CWD,
            check=False,
        ).returncode
    else:
        code = 0
    if code == 0:
        vm_host.prioritize(vm)
    return code


def run_count(
    cargo: list[str],
    name: str,
    *,
    then: list[list[str]] | None = None,
    capture: bool = True,
) -> tuple[int, str]:
    runner = locked_version((paths.ENGINE / "Cargo.lock").read_text(), "gungraun")
    if runner is None:
        log("no gungraun in engine/Cargo.lock")
        return 2, ""
    mounted = vm_host.vm_sees(paths.ENGINE, VM)
    script = count_script(
        provision_script(toolchain(), runner),
        cargo,
        name,
        str(paths.REPO) if mounted else f"streamed:{paths.REPO}",
        paths.worktree_names(),
        then=then,
        source="" if mounted else vm_host.bench_source_script(),
    )
    if mounted:
        run = subprocess.run(
            vm_command(script),
            cwd=vm_host.shell_host_cwd(),
            stdout=subprocess.PIPE if capture else None,
            text=True,
            check=False,
        )
    else:
        run = vm_host.run_with_sources(
            vm_command(script, workdir="/"), COUNT_SOURCES, capture=capture
        )
    return run.returncode, run.stdout or ""


def bench_count(args: argparse.Namespace) -> int:
    code = vm_ensure()
    if code != 0:
        return code
    cargo = cargo_bench(args.extra or COUNT_BENCHES)
    return run_count(cargo, paths.REPO.name, capture=False)[0]


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


if __name__ == "__main__":
    from .count_gate import main

    sys.exit(main(sys.argv[1:]))
