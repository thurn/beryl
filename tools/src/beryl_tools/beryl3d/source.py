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


def take_in_script() -> str:
    """The script lines that read the tar stream from stdin into [`SOURCE`]."""
    return f"""rm -rf {INCOMING} && mkdir -p {INCOMING} {SOURCE}
tar -x -C {INCOMING}
rsync -r --checksum --delete {INCOMING}/ {SOURCE}/
"""


def run_with_sources(script: str) -> subprocess.CompletedProcess[str] | None:
    """Runs `script` in the machine with the sources on its stdin, so it must begin with
    [`take_in_script`]. Its result, or `None` if the tar stream failed too."""
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
    run = subprocess.run(
        ["limactl", "shell", "--workdir", "/", vm.NAME, "bash", "-lc", script],
        stdin=tar.stdout,
        stdout=subprocess.PIPE,
        text=True,
        check=False,
    )
    tar.wait()
    if tar.returncode != 0:
        return None
    return run
