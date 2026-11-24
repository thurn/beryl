"""`beryl3d-bench`, the Lima virtual machine (a WSL distribution on Windows) in which The client's
instruction and fragment counts are taken.

It is made and provisioned with bench.py's helpers, as the engine's `beryl-bench` is, and is a
machine of its own so that the graphics driver it installs cannot move the engine's counts and
neither count gate waits for the other (docs/beryl-3d.md, 'What is counted').
"""

from __future__ import annotations

import argparse
import shlex

from .. import bench
from . import paths
from .command import Command

NAME = "beryl3d-bench"
# The machine's size. The gates build with several compilers at once (`JOBS_SCRIPT`) and the
# count gate runs several boards under Callgrind at once. With 4 GiB it built with one
# compiler, since two of the main workspace's crates built together passed 4 GiB and the kernel
# killed one.
CPUS = 8
MEMORY_GIB = 12
# Script lines that let cargo run one compiler for each 3 GiB of the machine's memory, at least
# one. A machine made before it had `MEMORY_GIB` keeps its old size until it is next found
# stopped (`bench.vm_ensure`), and builds with fewer compilers until then.
JOBS_SCRIPT = (
    "export CARGO_BUILD_JOBS=$(awk '/MemTotal/ {n = int($2 / 3145728);"
    " print (n < 1) ? 1 : n}' /proc/meminfo)\n"
)

# Mesa's Vulkan drivers, among them lavapipe, the software driver the renderer draws on to
# count fragments, and the Vulkan loader that finds them. `pkg-config` and OpenSSL's headers are
# for `openssl-sys`, which the renderer's example links through `beryl_art`'s `ureq`.
GRAPHICS = ("mesa-vulkan-drivers", "libvulkan1", "pkg-config", "libssl-dev", "rsync")


def provision_script(channel: str) -> str:
    """Idempotent: what `beryl-bench` installs except the Gungraun runner, which the client does
    not use, then the graphics packages."""
    packages = " ".join(GRAPHICS)
    return bench.provision_script(channel, None) + (
        f"if ! dpkg -s {packages} >/dev/null 2>&1; then\n"
        "  sudo apt-get update -qq\n"
        f"  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {packages}\n"
        "fi\n"
        # Ubuntu upgrades its packages unattended, and an upgrade of Valgrind or glibc moved
        # every count between two runs of the same commit by more than the tolerance. The
        # machine's packages change only when this script installs them.
        "if systemctl is-enabled --quiet unattended-upgrades 2>/dev/null; then\n"
        "  sudo systemctl disable --now unattended-upgrades\n"
        "fi\n"
    )


def start() -> int:
    """Creates or starts the machine at its size: 0, or the code to exit with."""
    return bench.vm_ensure(NAME, CPUS, MEMORY_GIB)


def ensure() -> int:
    """Creates and starts the machine if needed, and provisions it: 0, or the code to exit
    with."""
    code = start()
    if code != 0:
        return code
    # From the machine's root: it may be called from a worktree that the machine does not
    # mount, so the checkout's own path does not exist there.
    return bench.vm_shell(provision_script(bench.toolchain()), NAME, "/")


def run_vm(args: argparse.Namespace) -> int:
    code = ensure()
    if code != 0 or not args.script:
        return code
    # Login shell in the checkout's engine/ directory, with cargo on the path.
    script = f'. "$HOME/.cargo/env"\n{shlex.join(args.script)}'
    return bench.vm_shell(script, NAME)


def vm_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "script",
        nargs=argparse.REMAINDER,
        help="a command to run in the machine, from the checkout's engine/ directory",
    )


COMMANDS: tuple[Command, ...] = (
    Command(
        "vm",
        "creates and provisions the beryl3d-bench count machine; runs a command there",
        (
            ("just client vm", "create, start and provision the machine"),
            (
                "just client vm vulkaninfo --summary",
                "run a command in the machine, from engine/",
            ),
        ),
        run_vm,
        vm_arguments,
    ),
)
