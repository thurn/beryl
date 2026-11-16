"""The Lima virtual machines' share of this Mac: their priority, CPU tokens and memory.

The engine's count gate runs in `beryl-bench` (4 CPUs, 4 GiB) and Beryl 3D's frame counts in
`beryl3d-bench` (8 CPUs, 12 GiB). On 2026-11-15 they escaped every way the machine shares its
cores and memory:

- **Priority.** The Beryl 3D sweep ran its slow part at utility priority (`system.utility`) so
  that every other process got the CPU first. But macOS starts a Lima machine's process, the
  Virtualization framework's `com.apple.Virtualization.VirtualMachine` service, through
  launchd, so it inherits nothing from the command that asked for it. `beryl3d-bench` ran at
  normal priority (31) on 245% CPU while the sweep that used it asked for low priority.
- **Tokens.** A command run outside a check, such as `just bench-count` or `just b3d vm`, held
  no CPU tokens (tokens.py) for the machine's cores.
- **Memory.** A machine stays up once started, and macOS gets back none of the memory its
  guest has touched. `beryl-bench` had been up for four days, holding 3.7 GB while idle, and
  `beryl3d-bench` held 10 GB.

So every command that starts or uses a machine goes through `join` (`bench.vm_ensure` calls
it). It records the command's process as a user of the machine, takes as many CPU tokens as
the machine has CPUs, held until the command exits, and then gives the machine's process the
command's priority (`prioritize`): `PRIO_DARWIN_BG` for low-priority work, through
`taskpolicy -b`, and normal otherwise. A command inside a check step's lease, or with the pool
off as low-priority work has it, takes no tokens of its own. The machine's process has one
priority for all its users: the last command to start using it sets it.

`stop_idle` stops a machine that has had no live user for [`IDLE_MINUTES`], which returns its
memory to macOS; booting it again takes about 10 s. `join` calls it for the other machines,
and slow CI after each run. Joining and stopping take the same lock, so a machine is never
stopped under a command that has just joined it.

Windows runs its machines as WSL distributions, which this module leaves alone.
"""

from __future__ import annotations

import argparse
import atexit
import os
import subprocess
import time
from pathlib import Path

from . import paths, system, tokens

# How long a machine may sit with no user before `stop_idle` stops it. Booting takes about
# 10 s, and the fast check's count gate runs several times an hour while sessions push Rust
# changes, so a shorter wait would boot the machine for most of them.
IDLE_MINUTES = 15

# The name of the file in a machine's folder whose modification time is when it last had no
# live user.
IDLE_FILE = "idle"

# The leases taken by `join`, kept until this process exits.
_LEASES: list[tokens.Lease] = []


def folder(vm: str) -> Path:
    path = paths.cache_root() / "vms" / vm
    (path / "users").mkdir(parents=True, exist_ok=True)
    return path


def known() -> list[str]:
    """The machines some command has joined on this Mac."""
    root = paths.cache_root() / "vms"
    return sorted(p.name for p in root.iterdir() if p.is_dir()) if root.is_dir() else []


def live_users(vm: str) -> list[int]:
    """The processes using `vm`, removing the files of those that have exited and marking when
    the machine was last left with none. Call under the machine's lock."""
    here = folder(vm)
    live: list[int] = []
    for path in (here / "users").iterdir():
        try:
            pid = int(path.name)
        except ValueError:
            continue
        if system.alive(pid):
            live.append(pid)
        else:
            path.unlink(missing_ok=True)
            (here / IDLE_FILE).touch()
    return live


def machine_process(vm: str) -> int | None:
    """The process running `vm`: the one that holds its disk image open."""
    disk = Path.home() / ".lima" / vm / "disk"
    found = subprocess.run(
        ["lsof", "-t", str(disk)], capture_output=True, text=True, check=False
    )
    for line in found.stdout.split():
        if line.isdigit():
            return int(line)
    return None


def low_priority() -> bool:
    """Whether this command runs as low-priority work (`system.LOW_ENV`)."""
    return os.environ.get("BERYL_LOW_PRIORITY") == "1"


def background() -> bool:
    """Whether this command is work nobody is waiting for: low-priority work, or a sweep's,
    whose steps take tokens of the `sweep` class (tokens.py). Such a run takes a machine only
    when no other run is waiting for it (`bench.locked`)."""
    return low_priority() or tokens.named_class() == "sweep"


def set_priority(vm: str, low: bool) -> None:
    """Gives `vm`'s process the background priority, or takes it away."""
    pid = machine_process(vm)
    if pid is not None:
        flag = "-b" if low else "-B"
        subprocess.run(
            ["taskpolicy", flag, "-p", str(pid)], capture_output=True, check=False
        )


def join(vm: str, cpus: int) -> None:
    """Records this process as a user of `vm` until it exits, and takes `cpus` CPU tokens for
    the machine's cores. Call before starting the machine; `prioritize` after."""
    if system.WINDOWS:
        return
    here = folder(vm)
    with system.file_lock(here / "lock"):
        live_users(vm)
        (here / "users" / str(os.getpid())).write_text("")
    lease = tokens.acquire(tokens.named_class(), cpus, command=f"vm {vm}")
    if lease is not None and lease.files:
        _LEASES.append(lease)
        atexit.register(lease.release)
        # Commands this process starts from now on run inside the machine's lease. Without
        # this, a Beryl 3D sweep step hung for two hours: `just b3d gate --gpu` and
        # `--counts` start the machine, whose lease takes every token the `sweep` class may
        # hold, and then build the recordings pack on the Mac, and that cargo command waited
        # for a token of its own that only this process's exit would free. The machine sits
        # idle while this process builds, so the two share the cores without contention.
        os.environ.update(lease.env())
    stop_idle(skip=vm)


def prioritize(vm: str) -> None:
    """Gives the started machine `vm` this command's priority."""
    if not system.WINDOWS:
        set_priority(vm, low_priority())


def running(vm: str) -> bool:
    listed = subprocess.run(
        ["limactl", "list", "--format", "{{.Name}} {{.Status}}"],
        capture_output=True,
        text=True,
        check=False,
    )
    return f"{vm} Running" in listed.stdout.splitlines()


def stop_idle(skip: str = "", now: float | None = None) -> list[str]:
    """Stops each machine, other than `skip`, that has had no live user for [`IDLE_MINUTES`]:
    the machines it stopped. A running machine with no record of when it was last left idle,
    such as one started before this module, gets one now and is stopped after the wait.
    """
    if system.WINDOWS:
        return []
    now = time.time() if now is None else now
    stopped: list[str] = []
    for vm in known():
        if vm == skip:
            continue
        here = folder(vm)
        with system.file_lock(here / "lock"):
            if live_users(vm) or not running(vm):
                continue
            idle = here / IDLE_FILE
            if not idle.exists():
                idle.touch()
                continue
            if now - idle.stat().st_mtime < IDLE_MINUTES * 60:
                continue
            code = subprocess.run(
                ["limactl", "stop", "--tty=false", vm], capture_output=True, check=False
            ).returncode
            if code == 0:
                stopped.append(vm)
    return stopped


def stop_command(_args: argparse.Namespace) -> int:
    for vm in stop_idle():
        print(f"stopped {vm}: no user for {IDLE_MINUTES} minutes")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "vms-stop-idle",
        help=f"stop the Lima machines no command has used for {IDLE_MINUTES} minutes",
    )
    p.set_defaults(func=stop_command)
