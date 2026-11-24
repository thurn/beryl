"""Run selected the client test binaries from Cargo's workspace build without nextest."""

import json
import subprocess
import sys
from pathlib import Path
from typing import Any

from . import paths

BUILD = [
    "test",
    "-q",
    "--workspace",
    "--no-run",
    "--message-format=json-render-diagnostics",
]


def run(arguments: list[str], packages: list[str], filters: list[str]) -> int:
    """Run the selected packages' libtest binaries from `cargo test --no-run`."""
    # Each workspace member lives in crates/<package>. Cargo reports both the manifest
    # path and the exact executable it built, even when the artifact is fresh.
    directories: set[Path] = set()
    for package in packages:
        directory = paths.WORKSPACE / "crates" / package
        if directory.name != package or not (directory / "Cargo.toml").is_file():
            print(f"error: no such the client package: {package}", file=sys.stderr)
            return 101
        directories.add(directory)

    built = subprocess.run(
        ["cargo", *arguments],
        cwd=paths.WORKSPACE,
        stdout=subprocess.PIPE,
        encoding="utf-8",
        check=False,
    )
    if built.returncode != 0:
        return built.returncode

    for line in built.stdout.splitlines():
        artifact: dict[str, Any] = json.loads(line)
        if (
            artifact["reason"] != "compiler-artifact"
            or not artifact["profile"]["test"]
            or not artifact["executable"]
        ):
            continue
        directory = Path(artifact["manifest_path"]).parent
        if directory in directories:
            tested = subprocess.run(
                [artifact["executable"], "--quiet", "--", *filters],
                cwd=directory,
                check=False,
            )
            if tested.returncode != 0:
                return tested.returncode
    return 0
