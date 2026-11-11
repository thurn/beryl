"""The `build-paths` check: no Rust file bakes the compiling worktree's path into a binary."""

from __future__ import annotations

import re
from pathlib import Path

from . import paths

# `env!` of a build path fixes the compiling worktree's absolute path into the binary. The warm
# cache clones `rust/target` into other worktrees with test binaries cargo counts as fresh, which
# would then run a deleted worktree's binary or read its files, and pass against the wrong code.
BAKED_PATH: re.Pattern[str] = re.compile(
    r'(?<!\w)(?:option_)?env!\s*\(\s*"(CARGO_BIN_EXE_\w+|CARGO_MANIFEST_DIR)"'
)
RUN_TIME_PATH = {
    "CARGO_BIN_EXE": "find the binary at run time, as beryl_cli/tests/exe/mod.rs does"
    " (std::env::current_exe)",
    "CARGO_MANIFEST_DIR": 'read std::env::var_os("CARGO_MANIFEST_DIR") at run time',
}


def baked_build_paths(directory: Path) -> list[str]:
    """Each `env!` of a build path in a Rust file under `directory`, except in a `//` comment, as
    `path:line: problem; alternative`."""
    found: list[str] = []
    for path in sorted(directory.rglob("*.rs")):
        for number, line in enumerate(path.read_text().splitlines(), 1):
            if line.lstrip().startswith("//"):
                continue
            for m in BAKED_PATH.finditer(line):
                var = m.group(1)
                alternative = RUN_TIME_PATH[
                    "CARGO_BIN_EXE" if var.startswith("CARGO_BIN_EXE_") else var
                ]
                found.append(
                    f"{path}:{number}: {m.group(0)}) bakes in this worktree's path;"
                    f" {alternative}"
                )
    return found


def step() -> int:
    found = baked_build_paths(paths.RUST / "crates")
    for place in found:
        print(f"build-paths: {place}")
    return 1 if found else 0
