"""The rust lane's tests step (checks.py): the workspace's tests under cargo-nextest, each test's
result in the run log (runlog.py), or only the tests a failed run named (reruns.failed_tests).
`by test-rust` runs the same tests the same way for a session at work.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from collections import defaultdict
from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import paths, runlog, tokens, verdicts, warm
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
    return packaged_tests_step(*args, fast=True)


def slow_rust_tests_step(*args: str) -> int:
    """Slow CI's tests step: every test but those it leaves to the fast check alone
    (verdicts.FAST_ONLY), of the packages `--packages=a,b` names, else of the workspace.
    """
    return packaged_tests_step(*args, fast=False, slow=True)


def packaged_tests_step(*args: str, fast: bool, slow: bool = False) -> int:
    named = [a.split("=", 1)[1] for a in args if a.startswith("--packages=")]
    only = [a for a in args if not a.startswith("--packages=")]
    packages = named[0].split(",") if named else None
    if packages:
        print(f"rust-tests: the packages the change can break: {' '.join(packages)}")
    return rust_tests_step(*only, fast=fast, slow=slow, packages=packages)


def rust_tests_step(
    *only: str,
    fast: bool = False,
    slow: bool = False,
    packages: list[str] | None = None,
) -> int:
    """The workspace's tests, or `only` those named `<binary> <test>` (`failed_tests`); `fast`
    leaves out verdicts.SLOW_ONLY's, and `slow` verdicts.FAST_ONLY's.
    cargo-nextest runs the test binaries in parallel (cargo test runs them one after another);
    it cannot list the harness-less `allocations` targets and runs no doctest, so those run
    beside it once everything is built: the `allocations` binaries that build made, and cargo
    test's doctests of the packages that hold any (`doctest_packages`), which are where a
    `compile_fail` block holds that one index type cannot pass for another. Plain cargo test where
    cargo-nextest is missing. Each test's result goes to the run log: nextest's from its JUnit
    report (rust/.config/nextest.toml), the `allocations` targets' and the doctests' as one
    check each."""
    scope = [a for p in packages for a in ("-p", p)] if packages else ["--workspace"]
    cargo = ["cargo", "test", "--locked", *scope, "-q"]
    if shutil.which("cargo-nextest") is None:
        # cargo test reports no test to the run log, so the step logs one `rust:tests` record
        # for all of them, and a flake or a slow test cannot be told apart from the rest.
        print(
            "rust-tests: cargo-nextest is missing, so the run log records no test by name; "
            "install it with `cargo install cargo-nextest --locked`",
            file=sys.stderr,
            flush=True,
        )
        return subprocess.run(cargo, cwd=paths.RUST, check=False).returncode
    started = time.perf_counter()
    build = subprocess.run(
        [*cargo, "--no-run", "--message-format=json-render-diagnostics"],
        cwd=paths.RUST,
        stdout=subprocess.PIPE,
        text=True,
        errors="replace",
        check=False,
    )
    # The build's share of the step, which says whether a slow run compiled or tested slowly.
    print(f"rust-tests: built in {time.perf_counter() - started:.0f} s", flush=True)
    if build.returncode != 0:
        return CANNOT_RUN
    # Every test runs past a failure, so that a rerun of those named failed covers the run:
    # nextest would otherwise cancel the rest unrun.
    every = NO_ALLOCATIONS
    left_out = (
        slow_only_filter(s.check for s in verdicts.SLOW_ONLY)
        if fast
        else slow_only_filter(verdicts.FAST_ONLY) if slow else ""
    )
    if left_out:
        every += f" & not ({left_out})"
    nextest: list[str] = [
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
    allocations = executables(build.stdout, "allocations")
    beside: dict[str, tuple[list[str], str]] = {
        "rust:allocations": (run_each(allocations), ALLOCATIONS_FAILED),
    }
    documented = doctest_packages(workspace_packages(), packages)
    if documented:
        beside["rust:doctests"] = (
            ["cargo", "test", "--locked", *scope_of(documented), "-q", "--doc"],
            DOCTESTS_FAILED,
        )
    if not allocations:
        # None of the packages has an `allocations` target.
        del beside["rust:allocations"]
    if slow:
        for check in [c for c in beside if c in verdicts.FAST_ONLY]:
            del beside[check]
    jobs = [nextest] if only else [nextest, *(cmd for cmd, _ in beside.values())]
    junit = nextest_junit()
    junit.unlink(missing_ok=True)

    # The step's lease was sized when it started, often beside the browser lanes, which finish
    # first: nextest runs as wide as the tokens free once the build is done allow (tokens.py,
    # `top_up`). The other jobs only build and run a few binaries, so keep the step's lease.
    extra = tokens.top_up(tokens.largest(tokens.pool_size()))
    wide: dict[str, str] | None = None
    if extra.size:
        threads = int(os.environ[tokens.LEASE_ENV]) + extra.size
        print(f"rust-tests: nextest runs {threads} wide", flush=True)
        wide = {**os.environ, "NEXTEST_TEST_THREADS": str(threads)}

    def run(cmd: list[str]) -> tuple[subprocess.CompletedProcess[str], float]:
        started = time.perf_counter()
        done = subprocess.run(
            cmd,
            cwd=paths.RUST,
            env=wide if cmd is nextest else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            check=False,
        )
        return done, time.perf_counter() - started

    with extra, ThreadPoolExecutor(max_workers=len(jobs)) as pool:
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


def executables(messages: str, target: str) -> list[str]:
    """The test binaries named `target` among the artifacts that `cargo test --no-run
    --message-format=json` reports in `messages`.

    The step runs these rather than `cargo test --test <target>`: cargo resolves the features of
    a build of one test target apart from the whole workspace's, since the other packages'
    dev-dependencies are left out, so the hashes of the card crates' build dependencies differ.
    After a change to the rules crate, that second build compiled the rules, card and search
    crates a second time, 60 to 80 CPU seconds that made the fast check go over its budget.
    """
    found: list[str] = []
    for line in messages.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            not isinstance(message, dict)
            or message.get("reason") != "compiler-artifact"
        ):
            continue
        executable = message.get("executable")
        if executable and (message.get("target") or {}).get("name") == target:
            found.append(str(executable))
    return sorted(found)


# Runs each test binary given to it in turn, and fails if any failed.
RUN_EACH = (
    "import subprocess, sys; "
    "sys.exit(int(any([subprocess.run([b]).returncode != 0 for b in sys.argv[1:]])))"
)


def run_each(binaries: list[str]) -> list[str]:
    """A command that runs each of `binaries` in turn and fails if any of them failed."""
    return [sys.executable, "-c", RUN_EACH, *binaries]


def scope_of(packages: list[str]) -> list[str]:
    """Cargo's flags that select `packages`."""
    return [a for p in packages for a in ("-p", p)]


# A Markdown code fence in a `///` or `//!` doc comment, and the tags after its backticks.
DOC_FENCE: re.Pattern[str] = re.compile(r"^\s*//[/!]\s*```\s*(.*?)\s*$")
# The tags that leave a fenced block a Rust doctest that rustdoc compiles (`ignore` and any
# other language, such as `text`, do not).
DOCTEST_TAGS: frozenset[str] = frozenset(
    {"", "rust", "compile_fail", "should_panic", "no_run", "test_harness"}
)


def has_doctest(source: str) -> bool:
    """Whether a Rust source file holds a doc-comment code block that rustdoc compiles."""
    opened = False
    for line in source.splitlines():
        fence = DOC_FENCE.match(line)
        if fence is None:
            continue
        if not opened:
            tags = [t.strip() for t in fence.group(1).split(",")]
            if all(t in DOCTEST_TAGS or t.startswith("edition") for t in tags):
                return True
        opened = not opened
    return False


def doctest_packages(
    packages: list[dict[str, object]], names: list[str] | None
) -> list[str]:
    """The packages among `names` (None: the workspace) whose library holds a doctest.

    `cargo test --doc` builds the library of every package it is given, in a configuration no
    other step builds, before it runs any doctest. Given the whole workspace, that took 60 to
    100 s of the fast check after a change to the kernel, to run the seven doctests in two
    crates; given only those two crates, it takes under 10 s."""
    found: list[str] = []
    for p in packages:
        name = str(p["name"])
        if names is not None and name not in names:
            continue
        for target in p.get("targets") or []:  # pyre-ignore[16]: JSON
            if not set(target.get("kind") or []) & {"lib", "rlib"}:
                continue
            if not target.get("doctest", True):
                continue
            root = Path(str(target.get("src_path"))).parent
            if any(
                has_doctest(f.read_text(encoding="utf-8", errors="replace"))
                for f in sorted(root.rglob("*.rs"))
            ):
                found.append(name)
                break
    return sorted(found)


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


# What only cargo test runs: doctests, and the flags after `--` that go to a test binary.
CARGO_ONLY = ("--doc", "--")
# What keeps cargo-nextest from the harness-less `allocations` targets, which it cannot list.
NO_ALLOCATIONS = "not binary(allocations)"


def session_command(args: list[str], nextest: bool) -> list[str] | None:
    """The command `by test-rust <args>` runs in `rust/`, or none for the whole workspace as the
    check runs it (`rust_tests_step`): cargo-nextest's for a package or a name filter, and
    cargo test's for what nextest cannot run (`--doc`, `--test allocations`, flags after `--`)
    and where cargo-nextest is missing. A filterset of the caller's own (`-E`) is theirs to
    keep off the `allocations` targets."""
    asks_allocations = any(
        a == "--test=allocations" or (a == "allocations" and before == "--test")
        for before, a in zip(["", *args], args)
    )
    if not nextest or asks_allocations or any(a in CARGO_ONLY for a in args):
        return ["cargo", "test", *args]
    if not args:
        return None
    own_filterset = any(a in ("-E", "--filterset") for a in args)
    return [
        "cargo",
        "nextest",
        "run",
        "--cargo-quiet",
        "--no-fail-fast",
        "--status-level=fail",
        "--final-status-level=fail",
        *([] if own_filterset else ["-E", NO_ALLOCATIONS]),
        *args,
    ]


def session_tests(args: argparse.Namespace) -> int:
    # A fresh worktree's first build clones the cached target, as the checks' does.
    warm.ensure_target(paths.REPO, tauri=False)
    cmd = session_command(args.extra, shutil.which("cargo-nextest") is not None)
    # The build and the tests run with as many threads as the machine's CPU tokens allow.
    lease = tokens.acquire("agent", tokens.largest(tokens.pool_size()))
    assert lease is not None
    with lease:
        os.environ.update(lease.env())
        if cmd is None:
            return rust_tests_step()
        return subprocess.run(cmd, cwd=paths.RUST, check=False).returncode


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "test-rust",
        help="the Rust tests in parallel, as the check runs them (-p <crate>, a name filter)",
    )
    p.set_defaults(func=session_tests, passthrough=True)
