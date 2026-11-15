"""Sending this checkout's sources into the `beryl3d-bench` machine, for the gates that build
there (`counts.py`, `gpu.py`).

The machine mounts only the main checkout, and the sweep checks its commit out elsewhere, so a
gate cannot build from the checkout's path. It sends the files its build reads ([`SOURCES`]) as
a tar stream on the script's stdin, and the script copies them over the machine's own copy,
[`SOURCE`], with `rsync --checksum`: a file whose content did not change keeps its time, so Cargo
rebuilds only what changed.
"""

from __future__ import annotations

import os
import subprocess

from .. import paths as repo_paths
from . import vm

# What the builds read, relative to the checkout.
SOURCES: tuple[str, ...] = (
    "rust",
    "data",
    "docs/sample-decks",
    "ui/src/generated",
    "ui/fixtures.json",
)
# In the machine: where the sources arrive, and the copy the gates build from.
INCOMING = "$HOME/b3d-incoming"
SOURCE = "$HOME/b3d-source"


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
    files = sources()
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
            "-",
        ],
        cwd=repo_paths.REPO,
        env={**os.environ, "COPYFILE_DISABLE": "1"},
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
    )
    stdin = tar.stdin
    assert stdin is not None
    stdin.write("\0".join(files).encode() + b"\0")
    stdin.close()
    # From the machine's root, since this checkout's path may not be mounted there.
    command = [
        "limactl",
        "shell",
        "--workdir",
        "/",
        vm.NAME,
        "bash",
        "-lc",
        stream_script(script),
    ]
    shell = subprocess.Popen(
        command, stdin=tar.stdout, stdout=subprocess.PIPE, text=True
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
