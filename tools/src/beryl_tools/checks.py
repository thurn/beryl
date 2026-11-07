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
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path

from . import cards, paths, warm

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
NPM_BIN: Path = paths.UI / "node_modules" / ".bin"
# The game screen's code: it reads what a decision asks from the protocol
# (`DecisionView.ask`), never from the card asking, so a new card needs no change here.
GAME_UI: Path = paths.UI / "src" / "game"


@dataclass
class Lane:
    name: str
    # Whether it builds, so needs warm caches; Markdown and Python lanes skip the clone.
    warm: bool = False
    steps: list[tuple[str, list[str], Path]] = field(default_factory=list)

    def step(self, name: str, cmd: list[str], cwd: Path = paths.REPO) -> Lane:
        self.steps.append((name, cmd, cwd))
        return self


@dataclass
class Outcome:
    lane: str
    ok: bool
    seconds: float
    log: str
    timings: list[str]


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
        self.wall_start: float = time.monotonic()
        self.last: float = self.wall_start
        self.adjusted: float = 0.0

    def now(self) -> float:
        with self.lock:
            t = time.monotonic()
            self.adjusted += (t - self.last) / stretch(os.getloadavg()[0], self.cpus)
            self.last = t
            return self.adjusted

    def wall(self) -> float:
        return time.monotonic() - self.wall_start


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
        self.running: set[subprocess.Popen[bytes]] = set()

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
                    start_new_session=True,
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
                    kill_tree(proc.pid)
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
                kill_tree(proc.pid)


def kill_tree(pid: int) -> None:
    """SIGKILLs the process group `pid` leads and every process under `pid`, including any
    that left the group (cargo-nextest gives each test its own)."""
    table = subprocess.run(
        ["ps", "-A", "-o", "pid=,ppid="], capture_output=True, text=True, check=False
    ).stdout
    children: dict[int, list[int]] = {}
    for row in table.splitlines():
        child, parent = map(int, row.split())
        children.setdefault(parent, []).append(child)
    tree: list[int] = []
    frontier = [pid]
    while frontier:
        frontier = [c for p in frontier for c in children.get(p, [])]
        tree += frontier
    try:
        os.killpg(pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    for p in tree:
        try:
            os.kill(p, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass


def run_lane(lane: Lane, script: str, env: dict[str, str], steps: Steps) -> Outcome:
    clock = steps.clock
    started = clock.now()
    log: list[str] = []
    timings: list[str] = []
    for name, cmd, cwd in lane.steps:
        t, wall = clock.now(), clock.wall()
        code, output = steps.run(cmd, cwd, env)
        seconds, wall = clock.now() - t, clock.wall() - wall
        if output.strip():
            log.append(output.rstrip())
        if code != 0:
            log.append(
                f"{script} {lane.name} {name}: "
                + (
                    f"TIMED OUT after {took(wall, seconds)} (deadline {steps.deadline:.0f} s)"
                    if code is None
                    else f"FAILED (exit {code}) after {took(wall, seconds)}"
                )
            )
            timings.append(f"{name} {'TIMED OUT' if code is None else 'FAILED'}")
            return Outcome(
                lane.name, False, clock.now() - started, "\n".join(log), timings
            )
        log.append(f"{script} {lane.name} {name}: {took(wall, seconds)}")
        timings.append(f"{name} {seconds:.0f} s")
    return Outcome(lane.name, True, clock.now() - started, "\n".join(log), timings)


def run_lanes(lanes: list[Lane], script: str, budget: int, strict: bool) -> int:
    steps = Steps(float(os.environ.get("CHECK_DEADLINE_S", 2 * budget)))
    if any(lane.warm for lane in lanes):
        warm.ensure()
    env = dict(os.environ)
    with ThreadPoolExecutor(max_workers=max(1, len(lanes))) as pool:
        try:
            outcomes = list(
                pool.map(lambda lane: run_lane(lane, script, env, steps), lanes)
            )
        # Ctrl-C reaches only this process, not the steps' sessions.
        except BaseException:
            steps.stop()
            raise
    for o in sorted(outcomes, key=lambda o: not o.ok):
        print(f"=== {script} {o.lane}: {'passed' if o.ok else 'FAILED'}\n{o.log}")
    for o in outcomes:
        print(f"{script}: {o.lane} {o.seconds:.0f} s ({', '.join(o.timings)})")
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
    one, five, fifteen = os.getloadavg()
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
    return (
        Lane("python")
        .step("black", [sys.executable, "-m", "black", "--check", "--quiet", "tools"])
        .step("pyre", ["pyre", "--noninteractive", "check"], paths.TOOLS)
        .step("pytest", [sys.executable, "-m", "beryl_tools.checks", "pytest"])
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
        .step("tests", [sys.executable, "-m", "beryl_tools.checks", "rust-tests"])
    )


def ui_lane() -> Lane:
    return (
        Lane("ui", warm=True)
        .step("prettier", [str(NPM_BIN / "prettier"), "--check", "."], paths.UI)
        .step("tsc", [str(NPM_BIN / "tsc"), "-b"], paths.UI)
        .step("card-names", [sys.executable, "-m", "beryl_tools.checks", "card-names"])
    )


def app_lane() -> Lane:
    return Lane("app", warm=True).step("smoke", [*BY, "smoke"])


def shots_lane() -> Lane:
    """Renders every screen in ui/shots.json headless; a page error or a crash fails it."""
    return Lane("shots", warm=True).step("ui-shots", [*BY, "ui-shots"])


def ui_test_lane() -> Lane:
    """Plays the game screen through fixtures it records apart from `shots`' (shots.py), so
    the two run at once; a page error or a failed interaction fails it."""
    return Lane("ui-test", warm=True).step("ui-test", [*BY, "ui-test"])


def oracle_lane() -> Lane:
    """Plays every card scenario in XMage and Forge (`by oracle`): a failure the divergence
    allowlist does not explain fails it, as does an oracle not built at its pin."""
    return Lane("oracle", warm=True).step("oracle", [*BY, "oracle"])


def counts_lane() -> Lane:
    """Counts the cube rollout benchmark's instructions in the bench VM (`by count-gate`) and
    fails a rise past the bar in `beryl_bench/count-gate.toml`. The VM builds into its own
    target dir, so it needs no warm clone."""
    return Lane("counts").step("count-gate", [*BY, "count-gate"])


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
    return run_lanes(lanes, "check", FAST_BUDGET, strict=False)


def slow(args: argparse.Namespace) -> int:
    lanes = {
        "repo": repo_lane(),
        "python": python_lane(),
        "rust": rust_lane(),
        "ui": ui_lane(),
        "app": app_lane(),
        "shots": shots_lane(),
        "ui-test": ui_test_lane(),
        "oracle": oracle_lane(),
        "counts": counts_lane(),
    }
    chosen = args.lanes or list(lanes)
    unknown = set(chosen) - set(lanes)
    if unknown:
        raise SystemExit(f"unknown lanes {sorted(unknown)}; lanes: {' '.join(lanes)}")
    budget = int(os.environ.get("CI_BUDGET_S", SLOW_BUDGET))
    return run_lanes([lanes[n] for n in chosen], "ci", budget, strict=True)


def card_names_in(directory: Path, names: Iterable[str]) -> list[str]:
    """Each place a file under `directory` names one of `names`, as `path:line: name`."""
    alternatives = "|".join(re.escape(n) for n in sorted(names, key=len, reverse=True))
    pattern = re.compile(rf"(?<!\w)(?:{alternatives})(?!\w)")
    found: list[str] = []
    for path in sorted(p for p in directory.rglob("*") if p.is_file()):
        for number, line in enumerate(path.read_text().splitlines(), 1):
            found += [f"{path}:{number}: {m.group(0)}" for m in pattern.finditer(line)]
    return found


def all_card_names() -> set[str]:
    """The cube's cards, and the cards and tokens of the card data, but a token named by one
    word, such as Copy, which is no name in prose."""
    names = set(cards.cube_names(cards.CUBE))
    data = json.loads(cards.OUTPUT.read_text())
    tokens = {t["name"] for t in data["tokens"]}
    for entry in [*data["cards"], *data["tokens"]]:
        names.add(entry["name"])
        names.update(face["name"] for face in entry.get("faces", []))
    return {n for n in names if " " in n or n not in tokens}


def card_names_step() -> int:
    found = card_names_in(GAME_UI, all_card_names())
    for place in found:
        print(f"card-names: {place}: the game screen names a card")
    return 1 if found else 0


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


def rust_tests_step() -> int:
    """The workspace's tests. cargo-nextest runs the test binaries in parallel (cargo test runs
    them one after another); it cannot list the harness-less `allocations` targets, so cargo
    test runs those beside it once everything is built. Plain cargo test where cargo-nextest is
    missing."""
    cargo = ["cargo", "test", "--locked", "--workspace", "-q"]
    if shutil.which("cargo-nextest") is None:
        return subprocess.run(cargo, cwd=paths.RUST, check=False).returncode
    build = subprocess.run([*cargo, "--no-run"], cwd=paths.RUST, check=False)
    if build.returncode != 0:
        return build.returncode
    nextest = [
        "cargo",
        "nextest",
        "run",
        "--locked",
        "--workspace",
        "--cargo-quiet",
        "--status-level=fail",
        "--final-status-level=fail",
        "-E",
        "not binary(allocations)",
    ]
    jobs = [nextest, [*cargo, "--test", "allocations"]]

    def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            cmd,
            cwd=paths.RUST,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            check=False,
        )

    with ThreadPoolExecutor(max_workers=len(jobs)) as pool:
        runs = list(pool.map(run, jobs))
    for r in runs:
        print(r.stdout.rstrip(), flush=True)
    return next((r.returncode for r in runs if r.returncode != 0), 0)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser("check", help="the fast check (enact push)").set_defaults(func=fast)
    p = sub.add_parser("ci", help="slow CI, or some of its lanes")
    p.add_argument("lanes", nargs="*")
    p.set_defaults(func=slow)


if __name__ == "__main__":
    steps = {
        "pytest": pytest_step,
        "rust-tests": rust_tests_step,
        "card-names": card_names_step,
        "build-paths": build_paths_step,
    }
    sys.exit(steps[sys.argv[1]]())
