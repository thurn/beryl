"""The checks. `by check` is the fast one `enact push` runs (budget 60 s); `by ci` is slow CI
on the staging tip (budget 300 s, and a run over it fails). Both run lanes in parallel, each a
sequence of timed steps, and print failed lanes last. A run has a hard deadline of twice its
budget ($CHECK_DEADLINE_S overrides it): a step still running then is killed, with everything it
started, and fails its lane.

Budgets and deadlines count load-adjusted seconds ([`Clock`]): while other work oversubscribes
the machine (other sessions' checks, a game, `just longthink`), the clock runs slower, so a run
is held to what it would take on a machine to itself, and a hung step still dies.

The fast check runs the lanes whose files changed since the branch left origin/staging. Slow
CI runs every lane plus `app` (the debug app built and launched headless until its UI loads),
`shots` (the screens rendered headless), `ui-test` (the game screen played headless),
`oracle` (the card scenarios checked against XMage and Forge) and `counts` (the rollout
benchmark's instruction count held to its bar).

A step that fails is rerun once before its lane fails (plan K21), narrowed to the checks it says
failed where it can name them: a pass on the rerun passes the lane and prints a `flaky: <lane>
<check>` line for each. A step killed at the deadline, or one that failed to build or start
(exit `CANNOT_RUN`), is no flake and is not rerun, nor are the static checks (fmt, clippy, tsc).
After the run, each failure it resolves gets a verdict (verdicts.py), and a step whose only
failing checks are quarantined or pending fails no lane.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor
from typing import Any
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from pathlib import Path

from . import card_names, paths, runlog, system, verdicts, warm

FAST_BUDGET = 60
SLOW_BUDGET = 300
# The most a busy machine slows a run's clock (`Clock`): a step that hangs dies by this many
# times its deadline, however loaded the machine.
MAX_STRETCH = 8.0
COMMIT: re.Pattern[str] = re.compile(
    r"^(feat|fix|perf|refactor|test|docs|build|ci|chore|style|revert)"
    r"(\([a-z0-9-]+\))?!?: \S"
)
COMMIT_MAX = 60
TEXT_SUFFIXES = (".py", ".rs", ".md", ".toml", ".json", ".ts", ".tsx", ".css", ".html")

BY: list[str] = [sys.executable, "-m", "beryl_tools.cli"]
# A step's exit code when it failed to build or start (a compile error, a UiError, an oracle not
# built at its pin): its checks never ran, so a failure on rerun would only cost the time.
CANNOT_RUN = 125

# Given a failed step's output, the checks it names as failed, each an argument the step takes
# to run only that check; none to rerun the step whole.
Failed = Callable[[str], list[str]]


def whole(output: str) -> list[str]:
    """A step that cannot name its failed checks: it reruns whole."""
    return []


@dataclass
class Step:
    name: str
    cmd: list[str]
    cwd: Path
    # How the step reruns once after failing; None for a step whose failure is no flake.
    failed: Failed | None = None


@dataclass
class Lane:
    name: str
    # Whether it builds, so needs warm caches; Markdown and Python lanes skip the clone.
    warm: bool = False
    steps: list[Step] = field(default_factory=list)

    def step(
        self,
        name: str,
        cmd: list[str],
        cwd: Path = paths.REPO,
        rerun: Failed | None = None,
    ) -> Lane:
        self.steps.append(Step(name, cmd, cwd, rerun))
        return self


@dataclass
class Outcome:
    lane: str
    ok: bool
    seconds: float
    log: str
    timings: list[str]
    # The checks that failed and then passed on the rerun; a step's name where it reran whole.
    flaky: list[str] = field(default_factory=list)
    # Failures that failed no step: why (quarantined or pending), and the check.
    excused: list[tuple[str, str]] = field(default_factory=list)


def stretch(load: float, cpus: int) -> float:
    """How many times longer than on a machine to itself work takes under `load` (the run
    queue's average length) on `cpus` cores: the cores are shared among that many threads,
    so a step gets about `cpus / load` of what it would, however many threads it runs (up to
    the cores). Its own threads count too, so a check that alone oversubscribes the machine
    gets back at most that much.
    """
    return min(MAX_STRETCH, max(1.0, load / cpus))


class Clock:
    """Seconds since it started, as a machine to itself would have spent them: each stretch of
    wall time divided by how much the load then slowed it (`stretch`). It reads the load as
    it is asked the time, which a running step does every half second."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.cpus: int = os.cpu_count() or 1
        # perf_counter, not monotonic: on Windows monotonic ticks every 15.6 ms.
        self.wall_start: float = time.perf_counter()
        self.last: float = self.wall_start
        self.adjusted: float = 0.0

    def now(self) -> float:
        with self.lock:
            t = time.perf_counter()
            self.adjusted += (t - self.last) / stretch(system.loadavg()[0], self.cpus)
            self.last = t
            return self.adjusted

    def wall(self) -> float:
        return time.perf_counter() - self.wall_start


def took(wall: float, adjusted: float) -> str:
    """A duration, with the load-adjusted one where the load stretched it."""
    if wall <= adjusted * 1.1:
        return f"{wall:.1f} s"
    return f"{adjusted:.1f} s ({wall:.0f} s on the clock, the machine loaded)"


class Steps:
    """A run's steps, each started in its own session, so that once the run's deadline passes,
    or the run is interrupted, a step can be killed with everything it started."""

    def __init__(self, deadline: float) -> None:
        self.deadline = deadline
        self.clock = Clock()
        self.stopped = threading.Event()
        self.lock = threading.Lock()
        # Popen's type depends on its arguments, which pyre reads differently per platform.
        self.running: set[subprocess.Popen[Any]] = set()

    def run(
        self, cmd: list[str], cwd: Path, env: dict[str, str]
    ) -> tuple[int | None, str]:
        """The step's exit code, None when the deadline killed it, and its output. The output
        goes to a file, not a pipe, so a killed step's straggler cannot hold up reading it.
        """
        with tempfile.TemporaryFile("w+", errors="replace") as out:
            with self.lock:
                if self.stopped.is_set() or self.clock.now() >= self.deadline:
                    return None, ""
                proc = subprocess.Popen(
                    cmd,
                    cwd=cwd,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    stdout=out,
                    stderr=subprocess.STDOUT,
                    **system.own_session(),
                )
                self.running.add(proc)
            code: int | None = None
            try:
                while (left := self.deadline - self.clock.now()) > 0:
                    try:
                        code = proc.wait(timeout=min(0.5, left))
                        break
                    except subprocess.TimeoutExpired:
                        continue
                else:
                    system.kill_tree(proc.pid)
                    proc.wait()
            finally:
                with self.lock:
                    self.running.discard(proc)
            out.seek(0)
            return code, out.read()

    def stop(self) -> None:
        """Kills every running step and starts no more."""
        with self.lock:
            self.stopped.set()
            for proc in self.running:
                system.kill_tree(proc.pid)


def run_lane(lane: Lane, run: runlog.Run, env: dict[str, str], steps: Steps) -> Outcome:
    """Runs the lane's steps until one fails, logging each attempt's checks (runlog.py): a
    rerun's own, so a check that failed and then passed shows as both, and the failure is
    marked flaky for its verdict. A step fails no lane when each check it failed is quarantined
    or pending (verdicts.excused), unless it never ran its checks."""
    script: str = run.script
    clock: Clock = steps.clock
    started = clock.now()
    log: list[str] = []
    timings: list[str] = []
    flaky: list[str] = []
    excused: list[tuple[str, str]] = []

    def attempt(
        step: Step, cmd: list[str]
    ) -> tuple[int | None, list[dict[str, object]]]:
        """Runs `cmd` for `step`, logging its output and how it ended; its exit code and the
        records it logged."""
        t, wall = clock.now(), clock.wall()
        with run.step(env) as (step_env, results):
            code, output = steps.run(cmd, step.cwd, step_env)
            seconds, wall = clock.now() - t, clock.wall() - wall
            records = run.log(lane.name, step.name, code == 0, wall, results)
        if output.strip():
            log.append(output.rstrip())
        ended = f"{script} {lane.name} {step.name}: "
        if code is None:
            ended += f"TIMED OUT after {took(wall, seconds)} (deadline {steps.deadline:.0f} s)"
        elif code != 0:
            ended += f"FAILED (exit {code}) after {took(wall, seconds)}"
        else:
            ended += took(wall, seconds)
        if code != 0 and step.failed and code not in (None, CANNOT_RUN):
            # Named before the rerun's output follows, so a rerun of the lot says so.
            checks = step.failed(output)
            ended += f"; rerunning {', '.join(checks) if checks else 'it'} once"
            log.append(ended)
            again, rerun = attempt(Step(step.name, step.cmd, step.cwd), cmd + checks)
            if again == 0:
                flaky.extend(checks or [step.name])
                for r in records:
                    r["flaky"] = r["outcome"] == "fail"
            return again, rerun
        log.append(ended)
        return code, records

    for step in lane.steps:
        t, flakes = clock.now(), len(flaky)
        code, records = attempt(step, step.cmd)
        why = verdicts.excused(records) if code not in (0, None, CANNOT_RUN) else []
        if why:
            excused.extend(why)
            held = ", ".join(f"{check} {reason}" for reason, check in why)
            log.append(f"{script} {lane.name} {step.name}: fails no lane: {held}")
            timings.append(f"{step.name} {clock.now() - t:.0f} s, excused")
            continue
        if code != 0:
            timings.append(f"{step.name} {'TIMED OUT' if code is None else 'FAILED'}")
            return Outcome(
                lane.name,
                False,
                clock.now() - started,
                "\n".join(log),
                timings,
                flaky,
                excused,
            )
        rerun = " after a rerun" if len(flaky) > flakes else ""
        timings.append(f"{step.name} {clock.now() - t:.0f} s{rerun}")
    return Outcome(
        lane.name, True, clock.now() - started, "\n".join(log), timings, flaky, excused
    )


# What each rerunnable step names as failed (`Failed`).

# nextest's line for a test that failed, crashed or timed out: its binary and its name.
NEXTEST_FAILED: re.Pattern[str] = re.compile(
    r"^\s*(?:TRY \d+ )?(?:FAIL|LEAK-FAIL|SIG[A-Z]+|ABORT|TIMEOUT) \[[^\]]*\]"
    r"(?: \(\s*\d+/\d+\))? (\S+) (\S+)\s*$",
    re.MULTILINE,
)
ALLOCATIONS_FAILED = "rust-tests: allocations FAILED"


def failed_tests(output: str) -> list[str]:
    """The tests nextest failed, each as `<binary> <test>` (`rust_tests_step` takes them); none
    when the allocations harness failed, which nextest does not run."""
    if ALLOCATIONS_FAILED in output:
        return []
    return list(dict.fromkeys(" ".join(m) for m in NEXTEST_FAILED.findall(output)))


def failed_shots(output: str) -> list[str]:
    """The shots `ui-shots` names on its `ui-shots: <shot>: <errors>` lines."""
    names = {
        s["name"] for s in json.loads((paths.UI / "shots.json").read_text())["shots"]
    }
    found = re.findall(r"^ui-shots: ([^\s:]+): ", output, re.MULTILINE)
    return [n for n in dict.fromkeys(found) if n in names]


def failed_titles(output: str) -> list[str]:
    """The checks `ui-test` names on its `FAILED: <title>` lines; none when it also reports
    page errors or sounds, which a rerun of those checks alone would not see again."""
    if re.search(r"^(page errors|sound):", output, re.MULTILINE):
        return []
    return list(dict.fromkeys(re.findall(r"^FAILED: (.+)$", output, re.MULTILINE)))


def failed_cards(output: str) -> list[str]:
    """The cards whose scenarios failed in the JSON summary `oracle` ends with."""
    start = re.search(r"^\{$", output, re.MULTILINE)
    try:
        summary = json.loads(output[start.start() :]) if start else {}
    except json.JSONDecodeError:
        return []
    cards = [
        str(s["card"])
        for result in summary.values()
        if isinstance(result, dict)
        for s in result.get("scenarios", [])
        if s.get("status") == "fail" or s.get("stale_allowlist")
    ]
    return list(dict.fromkeys(cards))


def run_lanes(lanes: list[Lane], script: str, budget: int, strict: bool) -> int:
    steps = Steps(float(os.environ.get("CHECK_DEADLINE_S", 2 * budget)))
    if any(lane.warm for lane in lanes):
        warm.ensure()
    env = dict(os.environ)
    run = runlog.Run.start(script)
    with ThreadPoolExecutor(max_workers=max(1, len(lanes))) as pool:
        try:
            outcomes = list(
                pool.map(lambda lane: run_lane(lane, run, env, steps), lanes)
            )
        # Ctrl-C reaches only this process, not the steps' sessions.
        except BaseException:
            steps.stop()
            raise
    for o in sorted(outcomes, key=lambda o: not o.ok):
        print(f"=== {script} {o.lane}: {'passed' if o.ok else 'FAILED'}\n{o.log}")
    for o in outcomes:
        print(f"{script}: {o.lane} {o.seconds:.0f} s ({', '.join(o.timings)})")
    # Checks that failed once and passed on the rerun (`run_lane`), for whoever greps the log.
    for o in outcomes:
        for check in o.flaky:
            print(f"flaky: {o.lane} {check}")
        for why, check in o.excused:
            print(f"{why}: {o.lane} {check}")
    for line in run.settle():
        print(line)
    elapsed = steps.clock.now()
    failed = [o.lane for o in outcomes if not o.ok]
    over = elapsed > budget
    print(
        f"{script}: {'FAILED: ' + ' '.join(failed) if failed else 'passed'} in {took(steps.clock.wall(), elapsed)} (budget {budget} s)"
    )
    if over:
        print(
            f"{script}: over budget: speed up or delete the slowest step"
            + (" (this fails slow CI)" if strict else "")
        )
        # A run slowed by other work (a game, `just longthink` at full power) looks like a slow
        # step in every lane at once; the load says which it was before anyone bisects.
        print(f"{script}: {load_note()}")
    return 1 if failed or (over and strict) else 0


def load_note() -> str:
    one, five, fifteen = system.loadavg()
    return (
        f"load average {one:.0f} {five:.0f} {fifteen:.0f} on {os.cpu_count()} cores;"
        f" the times above are adjusted for load beyond the cores, up to {MAX_STRETCH:.0f}x"
    )


# What changed.


def git(*args: str) -> str:
    run = subprocess.run(
        ["git", *args], cwd=paths.REPO, capture_output=True, text=True, check=False
    )
    return run.stdout.strip() if run.returncode == 0 else ""


def branch_base() -> str:
    for ref in ("origin/staging", "staging", "origin/master"):
        base = git("merge-base", ref, "HEAD")
        if base:
            return base
    return git("rev-parse", "--verify", "--quiet", "HEAD^")


def changed_paths(base: str) -> list[str] | None:
    """Paths changed since `base`, committed or not; None when there is no base."""
    if not base:
        return None
    diff = git("diff", "--name-only", "--no-renames", base).splitlines()
    untracked = git("ls-files", "--others", "--exclude-standard").splitlines()
    return sorted(set(diff + untracked))


def conflict_markers(files: list[str]) -> list[str]:
    hits = []
    for rel in files:
        f = paths.REPO / rel
        if not f.is_file() or not rel.endswith(TEXT_SUFFIXES):
            continue
        for i, line in enumerate(f.read_text(errors="replace").splitlines(), 1):
            if re.match(r"^(<{7}|>{7}|={7})( |$)", line):
                hits.append(f"{rel}:{i}: conflict marker")
    return hits


def commit_problems(base: str) -> list[str]:
    log = (
        git("log", "--no-merges", "--format=%h%x09%s", f"{base}..HEAD") if base else ""
    )
    problems = []
    for row in log.splitlines():
        sha, _, subject = row.partition("\t")
        if not COMMIT.match(subject):
            problems.append(
                f"{sha} {subject!r}: not a Conventional Commit (`fix: ...`)"
            )
        elif len(subject) > COMMIT_MAX:
            problems.append(f"{sha} {subject!r}: over {COMMIT_MAX} characters")
    return problems


def markdown_files() -> list[str]:
    """Every Markdown file but the beads' and the agent skills' (whose YAML front matter
    mdformat would rewrite)."""
    return [
        p
        for p in git("ls-files", "-co", "--exclude-standard", "*.md").splitlines()
        if not p.startswith((".beads/", ".agents/"))
    ]


# Lanes.


def repo_lane() -> Lane:
    return (
        Lane("repo")
        .step("just", ["just", "--fmt", "--check", "--unstable"])
        .step(
            "markdown",
            [sys.executable, "-m", "mdformat", "--check", *markdown_files()],
        )
    )


def python_lane() -> Lane:
    lane = Lane("python").step(
        "black", [sys.executable, "-m", "black", "--check", "--quiet", "tools"]
    )
    # Pyre has no Windows build: macOS checks the types.
    if not system.WINDOWS:
        lane.step("pyre", ["pyre", "--noninteractive", "check"], paths.TOOLS)
    return lane.step(
        "pytest", [sys.executable, "-m", "beryl_tools.checks", "pytest"], rerun=whole
    )


def rust_lane() -> Lane:
    return (
        Lane("rust", warm=True)
        .step(
            "build-paths", [sys.executable, "-m", "beryl_tools.checks", "build-paths"]
        )
        .step("fmt", ["cargo", "fmt", "--all", "--check"], paths.RUST)
        .step(
            "clippy",
            ["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
            paths.RUST,
        )
        .step(
            "tests",
            [sys.executable, "-m", "beryl_tools.checks", "rust-tests"],
            rerun=failed_tests,
        )
    )


def ui_lane() -> Lane:
    return (
        Lane("ui", warm=True)
        .step("prettier", [system.npm_bin("prettier"), "--check", "."], paths.UI)
        .step("tsc", [system.npm_bin("tsc"), "-b"], paths.UI)
    )


def names_lane() -> Lane:
    """No card name in generic code, held to the burn-down list (card_names.py)."""
    return Lane("names").step(
        "card-names", [sys.executable, "-m", "beryl_tools.checks", "card-names"]
    )


def app_lane() -> Lane:
    return Lane("app", warm=True).step("smoke", [*BY, "smoke"], rerun=whole)


def shots_lane() -> Lane:
    """Renders every screen in ui/shots.json headless; a page error or a crash fails it."""
    return Lane("shots", warm=True).step(
        "ui-shots", [*BY, "ui-shots"], rerun=failed_shots
    )


def ui_test_lane() -> Lane:
    """Plays the game screen through fixtures it records apart from `shots`' (shots.py), so
    the two run at once; a page error or a failed interaction fails it."""
    return Lane("ui-test", warm=True).step(
        "ui-test", [*BY, "ui-test"], rerun=failed_titles
    )


def oracle_lane() -> Lane:
    """Plays every card scenario in XMage and Forge (`by oracle`): a failure the divergence
    allowlist does not explain fails it, as does an oracle not built at its pin."""
    return Lane("oracle", warm=True).step("oracle", [*BY, "oracle"], rerun=failed_cards)


def mobile_lane() -> Lane:
    """Type-checks the app crate for the iOS Simulator and Android (`by mobile-check`); the
    phone builds themselves take minutes, too long for the budget."""
    return Lane("mobile", warm=True).step("mobile-check", [*BY, "mobile-check"])


def counts_lane() -> Lane:
    """Counts the cube rollout benchmark's instructions in the bench VM (`by count-gate`) and
    fails a rise past the bar in `beryl_bench/count-gate.toml`. The VM builds into its own
    target dir, so it needs no warm clone."""
    return Lane("counts").step("count-gate", [*BY, "count-gate"], rerun=whole)


def fast(args: argparse.Namespace) -> int:
    base = branch_base()
    changed: list[str] | None = changed_paths(base)
    every: bool = changed is None

    def touched(pattern: str) -> bool:
        return every or any(re.match(pattern, p) for p in changed or [])

    problems = conflict_markers(changed or []) + commit_problems(base)
    for p in problems:
        print(f"check: {p}")
    if problems:
        return 1
    print(
        f"check: {'every lane (no base)' if every else f'{len(changed or [])} paths changed since {base[:8]}'}"
    )
    lanes = [repo_lane()]
    if touched(r"^tools/"):
        lanes.append(python_lane())
    if touched(r"^rust/"):
        lanes.append(rust_lane())
    if touched(r"^ui/"):
        lanes.append(ui_lane())
    if touched(r"^(tools|rust|ui)/|^data/card-names-allowlist\.json$"):
        lanes.append(names_lane())
    return run_lanes(lanes, "check", FAST_BUDGET, strict=False)


def slow(args: argparse.Namespace) -> int:
    lanes = {
        "repo": repo_lane(),
        "python": python_lane(),
        "rust": rust_lane(),
        "ui": ui_lane(),
        "names": names_lane(),
        "app": app_lane(),
        "shots": shots_lane(),
        "ui-test": ui_test_lane(),
        "oracle": oracle_lane(),
        "counts": counts_lane(),
        "mobile": mobile_lane(),
    }
    if system.WINDOWS and not args.lanes:
        # The count gate's VM is Lima's, and the phone toolchains are set up, on macOS.
        print("ci: skipping counts and mobile, which need macOS")
        del lanes["counts"]
        del lanes["mobile"]
    chosen = args.lanes or list(lanes)
    unknown = set(chosen) - set(lanes)
    if unknown:
        raise SystemExit(f"unknown lanes {sorted(unknown)}; lanes: {' '.join(lanes)}")
    budget = int(os.environ.get("CI_BUDGET_S", SLOW_BUDGET))
    return run_lanes([lanes[n] for n in chosen], "ci", budget, strict=True)


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


def build_paths_step() -> int:
    found = baked_build_paths(paths.RUST / "crates")
    for place in found:
        print(f"build-paths: {place}")
    return 1 if found else 0


def pytest_step() -> int:
    code = subprocess.run(
        [sys.executable, "-m", "pytest", "-q"], cwd=paths.TOOLS, check=False
    ).returncode
    return 0 if code == 5 else code  # 5: no tests collected


def nextest_filter(tests: Iterable[str]) -> str:
    """A filterset of exactly `tests`, each `<binary> <test>` as nextest names a failure."""
    return " | ".join(
        f"(binary_id({binary}) & test(={name}))"
        for binary, name in (t.split(" ", 1) for t in tests)
    )


def rust_tests_step(*only: str) -> int:
    """The workspace's tests, or `only` those named `<binary> <test>` (`failed_tests`).
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
        nextest_filter(only) if only else "not binary(allocations)",
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


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser("check", help="the fast check (enact push)").set_defaults(func=fast)
    p = sub.add_parser("ci", help="slow CI, or some of its lanes")
    p.add_argument("lanes", nargs="*")
    p.set_defaults(func=slow)


if __name__ == "__main__":
    steps: dict[str, Callable[..., int]] = {
        "pytest": pytest_step,
        "rust-tests": rust_tests_step,
        "card-names": card_names.step,
        "build-paths": build_paths_step,
    }
    sys.exit(steps[sys.argv[1]](*sys.argv[2:]))
