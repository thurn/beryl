"""The Python tests as the fast check and slow CI run them."""

from __future__ import annotations

import subprocess
import sys

from . import paths


def pytest_step(*tests: str) -> int:
    """Pytest on `tests` (paths under tools/), or on all of them, in four processes
    (pytest-xdist): most of the suite waits on git and subprocesses, so it takes about half
    as long, and a loaded machine leaves more workers nothing to run on."""
    code = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-n", "4", *tests],
        cwd=paths.TOOLS,
        check=False,
    ).returncode
    return 0 if code == 5 else code  # 5: no tests collected
