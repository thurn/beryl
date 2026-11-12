"""The rust lane's tests step (checks.py): the workspace's tests under cargo-nextest, each test's
result in the run log (runlog.py), or only the tests a failed run named (reruns.failed_tests).
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import time
import xml.etree.ElementTree as ET
from collections import defaultdict
from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import paths, runlog, verdicts
from .reruns import ALLOCATIONS_FAILED, CANNOT_RUN, DOCTESTS_FAILED


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


def workspace_packages() -> list[dict[str, object]]:
    """The workspace's packages as `cargo metadata` describes them; none when it fails."""
    run = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd=paths.RUST,
        capture_output=True,
        text=True,
        check=False,
    )
    try:
        return list(json.loads(run.stdout)["packages"]) if run.returncode == 0 else []
    except (ValueError, KeyError):
        return []


def affected_packages(
    changed: Iterable[str] | None, packages: list[dict[str, object]] | None = None
) -> list[str] | None:
    """The packages a change can break, for the fast check's clippy and tests: each package it
    touched and every package that depends on one of them, through any kind of dependency.
    None for the whole workspace: no list of changes, a change under rust/ outside every
    package (the workspace's manifest, lock file or cargo configuration), the card store,
    which a crate embeds, or a change that reaches every package anyway. Most changes touch
    cards or rules, on which nearly everything depends; one to the search or the CLI alone
    builds and tests a third of the workspace."""
    if changed is None:
        return None
    packages = workspace_packages() if packages is None else packages
    dirs: dict[str, str] = {}
    dependents: defaultdict[str, set[str]] = defaultdict(set)
    for p in packages:
        manifest = Path(str(p["manifest_path"])).parent
        dirs[Path(os.path.relpath(manifest, paths.REPO)).as_posix() + "/"] = str(
            p["name"]
        )
        for d in p.get("dependencies") or []:  # pyre-ignore[16]: JSON
            if isinstance(d, dict) and d.get("path"):
                dependents[str(d["name"])].add(str(p["name"]))
    touched: set[str] = set()
    for path in changed:
        if path == "data/card-store.tsv":
            return None
        if not path.startswith("rust/"):
            continue
        owner = next((n for d, n in dirs.items() if path.startswith(d)), None)
        if owner is None:
            return None
        touched.add(owner)
    found, stack = set(touched), list(touched)
    while stack:
        for r in dependents[stack.pop()] - found:
            found.add(r)
            stack.append(r)
    return sorted(found) if found and len(found) < len(dirs) else None


def fast_rust_tests_step(*args: str) -> int:
    """The fast check's tests step: the tests but those the pruning rule moved to slow CI alone
    (verdicts.SLOW_ONLY), of the packages `--packages=a,b` names (`affected_packages`), else
    of the workspace."""
    named = [a.split("=", 1)[1] for a in args if a.startswith("--packages=")]
    only = [a for a in args if not a.startswith("--packages=")]
    packages = named[0].split(",") if named else None
    if packages:
        print(f"rust-tests: the packages the change can break: {' '.join(packages)}")
    return rust_tests_step(*only, fast=True, packages=packages)


def rust_tests_step(
    *only: str, fast: bool = False, packages: list[str] | None = None
) -> int:
    """The workspace's tests, or `only` those named `<binary> <test>` (`failed_tests`); `fast`
    leaves out verdicts.SLOW_ONLY's.
    cargo-nextest runs the test binaries in parallel (cargo test runs them one after another);
    it cannot list the harness-less `allocations` targets and runs no doctest, so cargo test
    runs those beside it once everything is built: the doctests are where a `compile_fail`
    block holds that one index type cannot pass for another. Plain cargo test where
    cargo-nextest is missing. Each test's result goes to the run log: nextest's from its JUnit
    report (rust/.config/nextest.toml), the `allocations` targets' and the doctests' as one
    check each."""
    scope = [a for p in packages for a in ("-p", p)] if packages else ["--workspace"]
    cargo = ["cargo", "test", "--locked", *scope, "-q"]
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
        *scope,
        "--cargo-quiet",
        "--no-fail-fast",
        "--status-level=fail",
        "--final-status-level=fail",
        "-E",
        nextest_filter(only) if only else every,
    ]
    # Beside nextest: what it does not run, with the check and the line a failure is named by.
    beside = {
        "rust:allocations": ([*cargo, "--test", "allocations"], ALLOCATIONS_FAILED),
        "rust:doctests": ([*cargo, "--doc"], DOCTESTS_FAILED),
    }
    if packages:
        # Cargo refuses a target that none of the packages it is given has.
        kinds = targets(workspace_packages(), packages)
        if "allocations" not in kinds:
            del beside["rust:allocations"]
        if "lib" not in kinds:
            del beside["rust:doctests"]
    jobs = [nextest] if only else [nextest, *(cmd for cmd, _ in beside.values())]
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
        for check, (command, failed) in beside.items():
            if cmd is command:
                if r.returncode != 0:
                    print(failed, flush=True)
                runlog.result(check, "pass" if r.returncode == 0 else "fail", seconds)
    try:
        for check, outcome, took in junit_results(junit.read_text()):
            runlog.result(check, outcome, took)
    except (OSError, ET.ParseError) as e:
        print(f"rust-tests: no per-test results for the run log: {e}")
    return next((r.returncode for r, _ in runs if r.returncode != 0), 0)


def targets(packages: list[dict[str, object]], names: list[str]) -> set[str]:
    """The kinds of the targets of the packages named `names`, and their test targets' names."""
    found: set[str] = set()
    for p in packages:
        if p["name"] not in names:
            continue
        for target in p.get("targets") or []:  # pyre-ignore[16]: JSON
            kinds = set(target.get("kind") or [])
            found |= {"lib"} if kinds & {"lib", "rlib"} else set()
            found |= {str(target.get("name"))} if "test" in kinds else set()
    return found


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
