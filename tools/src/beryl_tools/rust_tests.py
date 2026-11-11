"""The rust lane's tests step (checks.py): the workspace's tests under cargo-nextest, each test's
result in the run log (runlog.py), or only the tests a failed run named (reruns.failed_tests).
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import time
import xml.etree.ElementTree as ET
from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import paths, runlog, verdicts
from .reruns import ALLOCATIONS_FAILED, CANNOT_RUN


def nextest_filter(tests: Iterable[str]) -> str:
    """A filterset of exactly `tests`, each `<binary> <test>` as nextest names a failure."""
    return " | ".join(
        f"(binary_id({binary}) & test(={name}))"
        for binary, name in (t.split(" ", 1) for t in tests)
    )


def slow_only_filter(checks: Iterable[str]) -> str:
    """A filterset of the Rust tests among `checks` (`rust:<binary>::<test path>`, run-log ids).
    The id does not say where the binary ends: `<crate>` (unit tests) or `<crate>::<target>`
    (an integration test or a binary), so a test matches by its package and either test path it
    can have. A binary_id() that matches no binary is an error to nextest, and a test() is not,
    so the entry of a deleted test matches nothing; a deleted crate's must go."""
    found: list[str] = []
    for check in checks:
        lane, _, rest = check.partition(":")
        parts = rest.split("::")
        if lane != "rust" or len(parts) < 2:
            continue
        target = regex(parts[1]) + "::"
        name = regex("::".join(parts[2:])) if len(parts) > 2 else ""
        path = f"({target})?{name}" if name else regex(parts[1])
        found.append(f"(package({parts[0]}) & test(/^{path}$/))")
    return " | ".join(found)


def regex(text: str) -> str:
    """`text` as a literal in a nextest regex, which `/` delimits."""
    return re.escape(text).replace("/", "\\/")


def fast_rust_tests_step(*only: str) -> int:
    """The fast check's tests step: the workspace's tests but those the pruning rule moved to
    slow CI alone (verdicts.SLOW_ONLY)."""
    return rust_tests_step(*only, fast=True)


def rust_tests_step(*only: str, fast: bool = False) -> int:
    """The workspace's tests, or `only` those named `<binary> <test>` (`failed_tests`); `fast`
    leaves out verdicts.SLOW_ONLY's.
    cargo-nextest runs the test binaries in parallel (cargo test runs them one after another);
    it cannot list the harness-less `allocations` targets, so cargo test runs those beside it
    once everything is built. Plain cargo test where cargo-nextest is missing. Each test's result
    goes to the run log: nextest's from its JUnit report (rust/.config/nextest.toml), the
    `allocations` targets' as one check."""
    cargo = ["cargo", "test", "--locked", "--workspace", "-q"]
    if shutil.which("cargo-nextest") is None:
        return subprocess.run(cargo, cwd=paths.RUST, check=False).returncode
    build = subprocess.run([*cargo, "--no-run"], cwd=paths.RUST, check=False)
    if build.returncode != 0:
        return CANNOT_RUN
    # Every test runs past a failure, so that a rerun of those named failed covers the run:
    # nextest would otherwise cancel the rest unrun.
    every = "not binary(allocations)"
    slow_only = slow_only_filter(s.check for s in verdicts.SLOW_ONLY) if fast else ""
    if slow_only:
        every += f" & not ({slow_only})"
    nextest = [
        "cargo",
        "nextest",
        "run",
        "--locked",
        f"--profile={NEXTEST_PROFILE}",
        "--workspace",
        "--cargo-quiet",
        "--no-fail-fast",
        "--status-level=fail",
        "--final-status-level=fail",
        "-E",
        nextest_filter(only) if only else every,
    ]
    allocations = [*cargo, "--test", "allocations"]
    jobs = [nextest] if only else [nextest, allocations]
    junit = nextest_junit()
    junit.unlink(missing_ok=True)

    def run(cmd: list[str]) -> tuple[subprocess.CompletedProcess[str], float]:
        started = time.perf_counter()
        done = subprocess.run(
            cmd,
            cwd=paths.RUST,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            check=False,
        )
        return done, time.perf_counter() - started

    with ThreadPoolExecutor(max_workers=len(jobs)) as pool:
        runs = list(pool.map(run, jobs))
    for cmd, (r, seconds) in zip(jobs, runs):
        print(r.stdout.rstrip(), flush=True)
        if cmd is allocations:
            if r.returncode != 0:
                print(ALLOCATIONS_FAILED, flush=True)
            runlog.result(
                "rust:allocations", "pass" if r.returncode == 0 else "fail", seconds
            )
    try:
        for check, outcome, took in junit_results(junit.read_text()):
            runlog.result(check, outcome, took)
    except (OSError, ET.ParseError) as e:
        print(f"rust-tests: no per-test results for the run log: {e}")
    return next((r.returncode for r, _ in runs if r.returncode != 0), 0)


NEXTEST_PROFILE = "check"


def nextest_junit() -> Path:
    """Where nextest's `check` profile writes its JUnit report (rust/.config/nextest.toml)."""
    target = Path(os.environ.get("CARGO_TARGET_DIR") or paths.RUST / "target")
    return target / "nextest" / NEXTEST_PROFILE / "junit.xml"


def junit_results(xml: str) -> list[tuple[str, str, float]]:
    """Each test in nextest's JUnit report as a run-log check, `rust:<binary>::<test path>`,
    with its outcome and seconds. A test that failed and then passed on a retry passed.
    """
    found: list[tuple[str, str, float]] = []
    for case in ET.fromstring(xml).iter("testcase"):
        outcome = "pass"
        if case.find("failure") is not None or case.find("error") is not None:
            outcome = "fail"
        elif case.find("skipped") is not None:
            outcome = "skip"
        check = f"rust:{case.get('classname')}::{case.get('name')}"
        found.append((check, outcome, float(case.get("time") or 0)))
    return found
