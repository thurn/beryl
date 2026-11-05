"""The checks. `by check` is the fast one `enact push` runs (budget 60 s); `by ci` is slow CI
on the staging tip (budget 300 s, and a run over it fails). Both run lanes in parallel, each a
sequence of timed steps, and print failed lanes last.

The fast check runs the lanes whose files changed since the branch left origin/staging. Slow
CI runs every lane plus `app`: the debug app built and launched headless until its UI loads.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from . import paths, warm

FAST_BUDGET = 60
SLOW_BUDGET = 300
COMMIT: re.Pattern[str] = re.compile(
    r"^(feat|fix|perf|refactor|test|docs|build|ci|chore|style|revert)"
    r"(\([a-z0-9-]+\))?!?: \S"
)
COMMIT_MAX = 60
TEXT_SUFFIXES = (".py", ".rs", ".md", ".toml", ".json", ".ts", ".tsx", ".css", ".html")

BY: list[str] = [sys.executable, "-m", "beryl_tools.cli"]
NPM_BIN: Path = paths.UI / "node_modules" / ".bin"


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


def run_lane(lane: Lane, script: str, env: dict[str, str]) -> Outcome:
    started = time.monotonic()
    log: list[str] = []
    timings: list[str] = []
    for name, cmd, cwd in lane.steps:
        t = time.monotonic()
        run = subprocess.run(
            cmd,
            cwd=cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            check=False,
        )
        seconds = time.monotonic() - t
        if run.stdout.strip():
            log.append(run.stdout.rstrip())
        if run.returncode != 0:
            log.append(
                f"{script} {lane.name} {name}: FAILED (exit {run.returncode}) after {seconds:.0f} s"
            )
            timings.append(f"{name} FAILED")
            return Outcome(
                lane.name, False, time.monotonic() - started, "\n".join(log), timings
            )
        log.append(f"{script} {lane.name} {name}: {seconds:.1f} s")
        timings.append(f"{name} {seconds:.0f} s")
    return Outcome(lane.name, True, time.monotonic() - started, "\n".join(log), timings)


def run_lanes(lanes: list[Lane], script: str, budget: int, strict: bool) -> int:
    started = time.monotonic()
    if any(lane.warm for lane in lanes):
        warm.ensure()
    env = dict(os.environ)
    with ThreadPoolExecutor(max_workers=max(1, len(lanes))) as pool:
        outcomes = list(pool.map(lambda lane: run_lane(lane, script, env), lanes))
    for o in sorted(outcomes, key=lambda o: not o.ok):
        print(f"=== {script} {o.lane}: {'passed' if o.ok else 'FAILED'}\n{o.log}")
    for o in outcomes:
        print(f"{script}: {o.lane} {o.seconds:.0f} s ({', '.join(o.timings)})")
    elapsed = time.monotonic() - started
    failed = [o.lane for o in outcomes if not o.ok]
    over = elapsed > budget
    print(
        f"{script}: {'FAILED: ' + ' '.join(failed) if failed else 'passed'} in {elapsed:.0f} s (budget {budget} s)"
    )
    if over:
        print(
            f"{script}: over budget: speed up or delete the slowest step"
            + (" (this fails slow CI)" if strict else "")
        )
    return 1 if failed or (over and strict) else 0


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
        .step("fmt", ["cargo", "fmt", "--all", "--check"], paths.RUST)
        .step(
            "clippy",
            ["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
            paths.RUST,
        )
        .step("tests", ["cargo", "test", "--locked", "--workspace", "-q"], paths.RUST)
    )


def ui_lane() -> Lane:
    return (
        Lane("ui", warm=True)
        .step("prettier", [str(NPM_BIN / "prettier"), "--check", "."], paths.UI)
        .step("tsc", [str(NPM_BIN / "tsc"), "-b"], paths.UI)
    )


def app_lane() -> Lane:
    return Lane("app", warm=True).step("smoke", [*BY, "smoke"])


def shots_lane() -> Lane:
    """Renders every screen in ui/shots.json headless and plays the game screen through a
    fixture; a page error, a crash or a failed interaction fails it."""
    return (
        Lane("shots", warm=True)
        .step("ui-shots", [*BY, "ui-shots"])
        .step("ui-test", [*BY, "ui-test"])
    )


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
    }
    chosen = args.lanes or list(lanes)
    unknown = set(chosen) - set(lanes)
    if unknown:
        raise SystemExit(f"unknown lanes {sorted(unknown)}; lanes: {' '.join(lanes)}")
    budget = int(os.environ.get("CI_BUDGET_S", SLOW_BUDGET))
    return run_lanes([lanes[n] for n in chosen], "ci", budget, strict=True)


def pytest_step() -> int:
    code = subprocess.run(
        [sys.executable, "-m", "pytest", "-q"], cwd=paths.TOOLS, check=False
    ).returncode
    return 0 if code == 5 else code  # 5: no tests collected


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser("check", help="the fast check (enact push)").set_defaults(func=fast)
    p = sub.add_parser("ci", help="slow CI, or some of its lanes")
    p.add_argument("lanes", nargs="*")
    p.set_defaults(func=slow)


if __name__ == "__main__" and sys.argv[1:] == ["pytest"]:
    sys.exit(pytest_step())
