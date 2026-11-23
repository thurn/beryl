"""Sending this checkout's sources into the `beryl3d-bench` machine, for the gates that build
there (`counts.py`, `gpu.py`).

Lima mounts only the main checkout. WSL can see each worktree but builds faster on its Linux
filesystem. A gate sends the files its build reads ([`SOURCES`]) as a tar stream on the script's
stdin, and the script copies them over the machine's own copy,
[`SOURCE`], with `rsync --checksum`: a file whose content did not change keeps its time, so Cargo
rebuilds only what changed.
"""

from __future__ import annotations

import os
import shlex
import subprocess
import tempfile
from pathlib import Path

from .. import bench, system
from .. import paths as repo_paths
from .. import vm_host
from . import paths, vm

# What the builds read, relative to the checkout.
SOURCES: tuple[str, ...] = (
    # The workspace includes the toolkit crates and their bundled fonts.
    "rust",
    # Includes the native fixture definitions, tuning data, design tokens and audio assets.
    "data",
    "docs/sample-decks",
    "ui/src/generated",
)
# In the machine: where the sources arrive, and the copy the gates build from.
INCOMING = "$HOME/b3d-incoming"
SOURCE = "$HOME/b3d-source"
# Where in the machine's home the recordings pack arrives (`send_pack`), as `BERYL_CACHE` for
# the programs that read it.
VM_CACHE = "b3d-cache"


def sources() -> list[str]:
    """The files of [`SOURCES`] in this checkout, tracked or not, less those git ignores."""
    listing = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"]
        + list(SOURCES),
        cwd=repo_paths.REPO,
        capture_output=True,
        check=True,
    ).stdout.decode()
    return sorted(
        {f for f in listing.split("\0") if f and (repo_paths.REPO / f).is_file()}
    )


# The file descriptor on which a script run by [`run_with_sources`] finds the tar stream.
STREAM_FD = 3


def take_in_script() -> str:
    """The script lines that read the tar stream into [`SOURCE`]. GNU tar stops at the archive's
    end marker, so `cat` reads whatever padding follows it. The machine must read the whole
    stream, or the Mac's tar waits on the pipe for ever."""
    return f"""rm -rf {INCOMING} && mkdir -p {INCOMING} {SOURCE}
tar -x -C {INCOMING} <&{STREAM_FD}
cat <&{STREAM_FD} >/dev/null
exec {STREAM_FD}<&-
rsync -r --checksum --delete {INCOMING}/ {SOURCE}/
"""


def stream_script(script: str) -> str:
    """`script` with the tar stream moved from stdin to [`STREAM_FD`] and stdin made empty, so
    that only [`take_in_script`]'s lines read the stream. Any other command that reads stdin (an
    installer run while provisioning, say) would otherwise eat part of the archive."""
    return f"exec {STREAM_FD}<&0 </dev/null\n{script}"


def run_with_sources(script: str) -> subprocess.CompletedProcess[str] | None:
    """Runs `script` in the machine with the sources as a tar stream, which it reads with
    [`take_in_script`]. Its result, or `None` if only the tar stream failed."""
    if system.WINDOWS:
        # Build on the guest's Linux filesystem. The same tar receiver preserves the Cargo
        # cache even when a different Windows worktree supplies the sources.
        command = bench.vm_command(stream_script(script), vm.NAME, "/")
        return vm_host.run_with_sources(command, SOURCES, capture=True)
    # tar reads the file list from a file, not from a pipe this process writes. tar writes the
    # archive while it is still reading names, and nothing reads the archive until the machine's
    # shell starts below. With a list longer than a pipe's buffer, tar would block on its full
    # stdout and this process on tar's full stdin, for ever.
    with tempfile.TemporaryDirectory() as scratch:
        listing = Path(scratch) / "sources.list"
        listing.write_bytes("\0".join(sources()).encode() + b"\0")
        return stream_sources(script, str(listing))


def stream_sources(
    script: str, listing: str
) -> subprocess.CompletedProcess[str] | None:
    """[`run_with_sources`] for the NUL-separated file list in the file `listing`."""
    tar = subprocess.Popen(
        # macOS's tar would add an AppleDouble `._` file for each file with extended
        # attributes, which the card crates' build scripts would read as sources.
        [
            "tar",
            "-c",
            "-f",
            "-",
            "--no-xattrs",
            "--no-mac-metadata",
            "--null",
            "-T",
            listing,
        ],
        cwd=repo_paths.REPO,
        env={**os.environ, "COPYFILE_DISABLE": "1"},
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
    )
    # From the machine's root, since this checkout's path may not be mounted there.
    command = bench.vm_command(stream_script(script), vm.NAME, "/")
    shell = subprocess.Popen(
        command,
        cwd=vm_host.LIMA_HOST_CWD,
        stdin=tar.stdout,
        stdout=subprocess.PIPE,
        text=True,
    )
    # Only the shell may hold the pipe's read end. If this process kept it open too, a script
    # that stopped before reading the whole stream would leave tar blocked on a full pipe, and
    # `tar.wait()` would never return. Closed here, tar gets SIGPIPE and exits instead.
    assert tar.stdout is not None
    tar.stdout.close()
    out, _ = shell.communicate()
    tar.wait()
    run = subprocess.CompletedProcess(command, shell.returncode, out)
    # A failed script is the cause to report, and it alone explains a tar killed by SIGPIPE.
    if run.returncode != 0:
        return run
    if tar.returncode != 0:
        return None
    return run


def send_pack(pack: Path | None = None) -> bool:
    """Copies the recordings pack into the VM, building it on the host if the caller has not
    already done so. Its name is a hash of the sources so the program built in the VM from
    those sources finds it.
    """
    if pack is None:
        from .cli import (
            program,
            shared_build,
        )  # cli imports gate, which imports this module.

        if shared_build() != 0:
            return False
        built = subprocess.run(
            [str(program("record"))],
            cwd=paths.WORKSPACE,
            check=False,
            stdout=subprocess.PIPE,
            text=True,
        )
        lines = built.stdout.splitlines()
        if built.returncode != 0 or not lines:
            return False
        pack = Path(lines[-1])
    directory = f"{VM_CACHE}/b3d"
    if system.WINDOWS:
        # The guest receives bytes on stdin just like the source archive. Rename only when the
        # copy is complete, so another checkout using the same pack can never read half of it.
        name = shlex.quote(pack.name)
        script = f"""set -e
cd "$HOME"
mkdir -p {directory}
out=$(mktemp {directory}/.incoming.XXXXXX)
trap 'rm -f -- "$out"' EXIT
cat >"$out"
mv -f -- "$out" {directory}/{name}
"""
        with pack.open("rb") as data:
            return (
                subprocess.run(
                    bench.vm_command(script, vm.NAME, "/"), stdin=data, check=False
                ).returncode
                == 0
            )
    make = [
        "limactl",
        "shell",
        "--workdir",
        "/",
        vm.NAME,
        "bash",
        "-c",
        f'mkdir -p "$HOME/{directory}"',
    ]
    copy = ["limactl", "copy", str(pack.absolute()), f"{vm.NAME}:{directory}/"]
    return all(
        subprocess.run(cmd, cwd=vm_host.LIMA_HOST_CWD, check=False).returncode == 0
        for cmd in (make, copy)
    )
