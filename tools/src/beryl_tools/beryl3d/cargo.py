"""Run Cargo in the native client's workspace."""

import os
import subprocess

from .. import system
from . import paths


def run(arguments: list[str], env: dict[str, str] | None = None) -> int:
    """Runs cargo with `arguments` in Beryl 3D's workspace, in `env` (this process's own by
    default)."""
    return subprocess.run(
        ["cargo", *arguments],
        cwd=paths.WORKSPACE,
        env=os.environ if env is None else env,
        check=False,
    ).returncode


def build(arguments: list[str], env: dict[str, str] | None = None) -> int:
    """Runs `cargo <arguments>`, a build in the workspace's own target directory, one at a time
    across every process of this checkout. Cargo's lock on the directory already makes a second
    build wait while another runs. Taking a file lock first (`paths.build_lock`) makes the
    waiting explicit and means a second build usually finds nothing left to do."""
    with system.file_lock(paths.build_lock()):
        return run(arguments, env)
