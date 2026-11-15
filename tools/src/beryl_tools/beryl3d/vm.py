"""`beryl3d-bench`, the Lima virtual machine (a WSL distribution on Windows) in which Beryl 3D's
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

NAME = "beryl3d-bench"

# Mesa's Vulkan drivers, among them lavapipe, the software driver the renderer draws on to
# count fragments, and the Vulkan loader that finds them. `pkg-config` and OpenSSL's headers are
# for `openssl-sys`, which the renderer's example links through `beryl_art`'s `ureq`.
GRAPHICS = ("mesa-vulkan-drivers", "libvulkan1", "pkg-config", "libssl-dev")


def runner_version() -> str | None:
    """The Gungraun release the runner must match: Beryl 3D's lock file's, else the main
    workspace's, which the engine's machine also installs."""
    for lock in (paths.WORKSPACE / "Cargo.lock", bench.paths.RUST / "Cargo.lock"):
        found = bench.locked_version(lock.read_text(), "gungraun")
        if found is not None:
            return found
    return None


def provision_script(channel: str, runner: str) -> str:
    """Idempotent: what `beryl-bench` installs, then the graphics packages."""
    packages = " ".join(GRAPHICS)
    return bench.provision_script(channel, runner) + (
        f"if ! dpkg -s {packages} >/dev/null 2>&1; then\n"
        "  sudo apt-get update -qq\n"
        f"  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {packages}\n"
        "fi\n"
    )


def ensure() -> int:
    """Creates and starts the machine if needed, and provisions it: 0, or the code to exit
    with."""
    code = bench.vm_ensure(NAME)
    if code != 0:
        return code
    runner = runner_version()
    if runner is None:
        bench.log("no gungraun in rust/beryl3d/Cargo.lock or rust/Cargo.lock")
        return 2
    # From the machine's root: the sweep calls this from a worktree that the machine does not
    # mount, so the checkout's own path does not exist there.
    return bench.vm_shell(provision_script(bench.toolchain(), runner), NAME, "/")


def run_vm(args: argparse.Namespace) -> int:
    code = ensure()
    if code != 0 or not args.script:
        return code
    # Login shell in the checkout's rust/ directory, with cargo on the path.
    script = f'. "$HOME/.cargo/env"\n{shlex.join(args.script)}'
    return bench.vm_shell(script, NAME)


def vm_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "script",
        nargs=argparse.REMAINDER,
        help="a command to run in the machine, from the checkout's rust/ directory",
    )
