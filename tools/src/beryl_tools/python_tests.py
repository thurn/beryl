"""The Python tests as the fast check and slow CI run them.

Under a check (`$BERYL_CHECK_RESULTS` set), tests/conftest.py reports each test as a check named
`<lane>:<node id>`, such as `python:tests/test_warm.py::test_a_clone_is_kept`, so a failure or
flake names its test, and the step's one rerun runs only the tests that failed
(reruns.failed_pytests).
"""

from __future__ import annotations

import os
import subprocess
import sys

from . import paths, slow_only

# The environment variable that tells tests/conftest.py which lane its checks belong to.
LANE_ENV = "BERYL_PYTEST_LANE"


def pytest_step(*args: str) -> int:
    """Pytest on the tests in `args` (paths or node ids under tools/), or on all of them, in four
    processes (pytest-xdist): most of the suite waits on git and subprocesses, so it takes about
    half as long, and a loaded machine leaves more workers nothing to run on. `--lane <name>`
    first names the lane its checks are reported under (`python` by default). `--slow`, slow CI's,
    leaves out the tests slow_only.FAST_ONLY leaves to the fast check."""
    tests = list(args)
    lane = "python"
    if tests[:1] == ["--lane"]:
        lane, tests = tests[1], tests[2:]
    left_out: list[str] = []
    if "--slow" in tests:
        tests.remove("--slow")
        left_out = [a for t in slow_only.fast_only(lane) for a in ("--deselect", t)]
    code = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-n", "4", *left_out, *tests],
        cwd=paths.TOOLS,
        env={**os.environ, LANE_ENV: lane},
        check=False,
    ).returncode
    return 0 if code == 5 else code  # 5: no tests collected
