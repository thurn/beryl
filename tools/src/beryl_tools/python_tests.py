"""The Python tests as the fast check and slow CI run them.

Under a check (`$BERYL_CHECK_RESULTS` set), tests/conftest.py reports each test as a check named
`<lane>:<node id>`, such as `python:tests/test_warm.py::test_a_clone_is_kept`, so a failure or
flake names its test, and the step's one rerun runs only the tests that failed
(reruns.failed_pytests).
"""

from __future__ import annotations

import json
import os
import subprocess
import sys

from . import deadline, paths, python_fit, slow_only, system
from .changes import branch_base, changed_paths

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
    stop: float | None = None
    if "--slow" in tests:
        tests.remove("--slow")
        left_out = [a for t in slow_only.fast_only(lane) for a in ("--deselect", t)]
    elif (
        not tests and lane == "python" and (window := deadline.step_limit()) is not None
    ):
        fitted = fit(window)
        if fitted is not None:
            if not fitted:
                return 0
            tests = fitted
        stop = window - STOP_EARLY
    proc = subprocess.Popen(
        [sys.executable, "-m", "pytest", "-q", "-n", "4", *left_out, *tests],
        cwd=paths.TOOLS,
        env={**os.environ, LANE_ENV: lane},
        **system.own_session(),
    )
    try:
        code = proc.wait(timeout=None if stop is None else max(stop, 1.0))
    except subprocess.TimeoutExpired:
        system.kill_tree(proc.pid)
        proc.wait()
        return stopped(lane)
    return 0 if code == 5 else code  # 5: no tests collected


# The seconds before the end of its window (deadline.step_limit) at which the fast check's
# pytest step stops the tests still running, so that the step ends inside the window with the
# results of those that finished.
STOP_EARLY = 3.0


def stopped(lane: str) -> int:
    """The step's code after it stopped pytest at the end of its window: 1 when a test that
    finished failed (tests/conftest.py logged it to the run's results), else 0, the tests that
    did not finish being left to the sweep."""
    from . import runlog  # It imports checks, which imports this module.

    failed: list[str] = []
    results = os.environ.get(runlog.RESULTS_ENV)
    if results:
        try:
            with open(results) as f:
                lines = [json.loads(line) for line in f if line.strip()]
        except (OSError, ValueError):
            lines = []
        failed = [
            str(r.get("check"))
            for r in lines
            if r.get("outcome") == "fail"
            and str(r.get("check")).startswith(f"{lane}:tests/")
        ]
    print(
        f"pytest: stopped at the end of the step's window; the tests still running are"
        f" left to the sweep{f', and {len(failed)} finished with a failure' if failed else ''}",
        flush=True,
    )
    for check in failed:
        print(f"FAILED {check.removeprefix(f'{lane}:')}", flush=True)
    return 1 if failed else 0


def fit(window: float) -> list[str] | None:
    """Inside a fast check, the test files that fit the step's `window` seconds (python_fit.py),
    with the rest named as deferred to the sweep and logged as cut; None to run every file.
    """
    from . import runlog  # It imports checks, which imports this module.

    records = deadline.Records("python")
    chosen = python_fit.choose(changed_paths(branch_base()), records.get(), window)
    if chosen is None:
        return None
    run, cut = chosen
    if cut:
        print(python_fit.deferred_line(cut, window), flush=True)
        for name in cut:
            runlog.result(f"python:{name}", "skip", 0.0, "cut")
    return run
