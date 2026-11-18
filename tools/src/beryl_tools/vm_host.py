"""The Lima virtual machines' share of this Mac: their priority and memory.

The engine's count gate runs in `beryl-bench` (4 CPUs, 4 GiB) and Beryl 3D's frame counts in
`beryl3d-bench` (8 CPUs, 12 GiB). On 2026-11-15 they escaped two of the ways the machine shares
its cores and memory:

- **Priority.** A Beryl 3D run of low priority ran its slow part at utility priority (`system.utility`) so
  that every other process got the CPU first. But macOS starts a Lima machine's process, the
  Virtualization framework's `com.apple.Virtualization.VirtualMachine` service, through
  launchd, so it inherits nothing from the command that asked for it. `beryl3d-bench` ran at
  normal priority (31) on 245% CPU while the run that used it asked for low priority.
- **Memory.** A machine stays up once started, and macOS gets back none of the memory its
  guest has touched. `beryl-bench` had been up for four days, holding 3.7 GB while idle, and
  `beryl3d-bench` held 10 GB.

So every command that starts or uses a machine goes through `join` (`bench.vm_ensure` calls
it). It records the command's process as a user of the machine, and then gives the machine's
process the command's priority (`prioritize`): `PRIO_DARWIN_BG` for low-priority work, through
`taskpolicy -b`, and normal otherwise. The machine's process has one priority for all its
users: the last command to start using it sets it.

`stop_idle` stops a machine that has had no live user for [`IDLE_MINUTES`], which returns its
memory to macOS; booting it again takes about 10 s. `join` calls it for the other machines, and
`by vms-stop-idle` calls it on its own. Joining and stopping take the same lock, so a machine is
never stopped under a command that has just joined it.

Windows runs its machines as WSL distributions, which this module leaves alone.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time
from pathlib import Path

from . import paths, system
from .deps import CANNOT_RUN

# How long a machine may sit with no user before `stop_idle` stops it. Booting takes about
# 10 s, and the count gate runs several times an hour while sessions push Rust changes, so a
# shorter wait would boot the machine for most of them.
IDLE_MINUTES = 15

# The name of the file in a machine's folder whose modification time is when it last had no
# live user.
IDLE_FILE = "idle"


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
    """Whether this command is work nobody is waiting for, which is low-priority work. Such a
    run takes a machine only when no other run is waiting for it (`bench.locked`)."""
    return low_priority()


def set_priority(vm: str, low: bool) -> None:
    """Gives `vm`'s process the background priority, or takes it away."""
    pid = machine_process(vm)
    if pid is not None:
        flag = "-b" if low else "-B"
        subprocess.run(
            ["taskpolicy", flag, "-p", str(pid)], capture_output=True, check=False
        )


def join(vm: str) -> None:
    """Records this process as a user of `vm` until it exits. Call before starting the machine;
    `prioritize` after."""
    if system.WINDOWS:
        return
    here = folder(vm)
    with system.file_lock(here / "lock"):
        live_users(vm)
        (here / "users" / str(os.getpid())).write_text("")
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


def wsl_status(vm: str) -> str | None:
    """The WSL distribution `vm`'s state, Running or Stopped; None if there is none."""
    run = subprocess.run(
        ["wsl", "--list", "--verbose"],
        capture_output=True,
        text=True,
        env={**os.environ, "WSL_UTF8": "1"},
        check=False,
    )
    for line in run.stdout.splitlines():
        fields = line.replace("*", " ").split()
        if len(fields) >= 2 and fields[0] == vm:
            return fields[1]
    return None


def status(vm: str) -> str | None:
    """The machine `vm`'s state (Running, Stopped and so on); None if there is none."""
    if system.WINDOWS:
        return wsl_status(vm)
    listed = subprocess.run(
        ["limactl", "list", "--format", "{{.Name}} {{.Status}}"],
        capture_output=True,
        text=True,
        check=False,
    )
    for line in listed.stdout.splitlines():
        name, _, state = line.partition(" ")
        if name == vm:
            return state
    return None


def vm_mounts(vm: str) -> list[Path]:
    """The host directories Lima mounts in `vm`, from the `location:` lines of its
    lima.yaml; empty when the file cannot be read. The VM sees only these, so a checkout
    elsewhere (a second clone, say) cannot be built there."""
    try:
        text = (Path.home() / ".lima" / vm / "lima.yaml").read_text()
    except OSError:
        return []
    found = re.findall(
        r"^\s*-?\s*location:\s*[\"']?([^\"'#\n]+?)[\"']?\s*$", text, re.M
    )
    return [Path(os.path.expanduser(f)) for f in found if f.startswith(("/", "~"))]


def vm_sees(path: Path, vm: str) -> bool:
    """Whether `path` is inside a directory `vm` mounts. WSL sees every Windows drive, and a
    lima.yaml that cannot be read is given the benefit of the doubt."""
    if system.WINDOWS:
        return True
    mounts = vm_mounts(vm)
    resolved = path.resolve()
    return not mounts or any(resolved.is_relative_to(m.resolve()) for m in mounts)


def vm_failed(code: int, vm: str) -> int:
    """The code for a counting script that exited `code` (non-zero): `CANNOT_RUN` when the
    VM was the problem, ssh's 255 or a VM no longer running (one stopped as idle mid-run once
    failed the count gate with `ha.sock: no such file or directory` on
    2026-11-16), else `code`."""
    if code == 255 or status(vm) != "Running":
        message = f"the {vm} VM stopped or could not be reached (exit {code})"
        print(f"bench: {message}; nothing was counted", file=sys.stderr)
        return CANNOT_RUN
    return code
