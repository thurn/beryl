"""The checks. `by check` is the fast one `enact push` runs (budget 60 s); `by ci` is slow CI
on the staging tip (budget 300 s, 450 s on Windows). Both run lanes in parallel, each a
sequence of timed steps, and print failed lanes last. A run over its budget fails nothing: it
files a bead to audit its slowest steps (budget.py). A run has a hard deadline, twice its budget
and at least [`HANG_DEADLINE`] ($CHECK_DEADLINE_S overrides it), to catch a hang: a step still
running then is killed, with everything it started, and fails its lane.

Budgets and deadlines count load-adjusted seconds ([`Clock`]): while other work oversubscribes
the machine (other sessions' checks, a game, `just ladder`), the clock runs slower, so a run
is held to what it would take on a machine to itself, and a hung step still dies. Slow CI also
stops its steps on the wall clock before Enact's `slow_timeout` would kill it (bisect.time_left),
so a run the load stretched past that still prints its lanes and names the step it cut off.

The fast check runs the lanes whose files changed since the branch left origin/staging,
`oracle` on the scenarios of each card whose file changed, without Forge's AI games, and for a
change to ui/ or rust/, `ui-test` and `shots` on the browser checks it can break, as many as fit
its budget (selection.py). Slow CI runs every lane plus `app` (the debug app built and launched
headless until its UI loads), `shots` (the screens rendered headless), `ui-test` (the game screen
played headless), `oracle` (the card scenarios checked against XMage and Forge) and `counts` (the
rollout benchmark's instruction count held to its bar).

A step that fails is rerun once before its lane fails, narrowed to the checks it says
failed where it can name them: a pass on the rerun passes the lane and prints a `flaky: <lane>
<check>` line for each. A step killed at the deadline, or one that failed to build or start
(exit `CANNOT_RUN`), is no flake and is not rerun, nor are the static checks (fmt, clippy, tsc).
After the run, each failure it resolves gets a verdict (verdicts.py), and a step whose only
failing checks are quarantined or pending fails no lane.

Slow CI bisects a failure that survives its rerun over the commits since master (bisect.py).
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from typing import Any
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from . import (
    bench,
    board_door,
    build_paths,
    card_crates,
    card_files,
    card_header,
    card_names,
    card_text,
    citations,
    conventions,
    discarded_push,
    file_length,
    hygiene,
    mobile,
    paths,
    plan_references,
    python_tests,
    runlog,
    script_checks,
    speed,
    store_cards,
    system,
    tokens,
    two_player,
    verdicts,
    warm,
)
from . import budget as budget_beads
from .changes import branch_base, changed_paths, git, markdown_files, narrow
from .clock import MAX_STRETCH, Clock, WallClock, load_note, stretch, took
from .reruns import (
    CANNOT_RUN,
    Failed,
    failed_cards,
    failed_shots,
    failed_tests,
    failed_titles,
    unproven,
    whole,
)
from .rust_tests import (
    affected_packages,
    fast_rust_tests_step,
    junit_results,
    nextest_filter,
    rust_tests_step,
)

FAST_BUDGET = 60
# The least deadline: a hang guard, not a budget, so a slow machine's steps finish.
HANG_DEADLINE = 600
# Windows runs the same lanes markedly slower (process start, file system, antivirus).
SLOW_BUDGET = 450 if system.WINDOWS else 300
# The lanes Windows leaves to the Mac, which ships the app: the count gate's VM is Lima's and
# the phone toolchains are set up on macOS; the app and the browser lanes hung headless Chrome
# on Windows (by-xppd) and are the Mac's to judge. The oracle lane took 400-600 s there alone,
# over the budget, while its JVMs and its own build of the scenarios starved the rust lane
# beside it (by-50vi); what it checks, Beryl against XMage and Forge, is the same on either OS,
# and the fast check still runs it there on the cards a change touches. A Windows pass can then
# advance master with them unrun, until the Mac's next run catches a break.
WINDOWS_SKIPS: tuple[str, ...] = (
    "counts",
    "mobile",
    "app",
    "shots",
    "ui-test",
    "oracle",
)

BY: list[str] = [sys.executable, "-m", "beryl_tools.cli"]


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
class Failure:
    """A step that failed after its rerun, as bisect.py reruns it: the arguments that narrow it
    to the checks it named as failed (none to run it whole) and its last run's records.
    """

    step: Step
    args: list[str]
    records: list[dict[str, object]]
    # Whether its lane builds, so needs warm caches (`Lane.warm`).
    warm: bool = False


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
    # The step that failed the lane, when it is one a rerun can narrow (`Step.failed`).
    failure: Failure | None = None
    # Whether the run's deadline killed a step of the lane (`run_all` reruns it once).
    timed_out: bool = False
    # Whether that kill was the wall limit's (`Steps.walled`) rather than the deadline's.
    walled: bool = False


class Steps:
    """A run's steps, each started in its own session, so that once the run's deadline passes,
    or the run is interrupted, a step can be killed with everything it started."""

    def __init__(
        self,
        deadline: float,
        clock: Clock | None = None,
        wall_limit: float | None = None,
        klass: str = "agent",
    ) -> None:
        self.deadline = deadline
        self.klass = klass
        self.clock: Clock = clock or Clock()
        # Wall seconds (`Clock.wall`) before someone else's wall clock kills the run, such as
        # Enact's `slow_timeout`: a loaded machine stretches the deadline past it (by-tkgu),
        # and a run killed from outside prints nothing, so a step still running then is
        # killed here instead and named as timed out.
        self.wall_limit = wall_limit
        self.stopped = threading.Event()
        self.lock = threading.Lock()
        # Popen's type depends on its arguments, which pyre reads differently per platform.
        self.running: set[subprocess.Popen[Any]] = set()

    def left(self) -> float:
        """Seconds before the deadline or the wall limit, whichever comes first."""
        left = self.deadline - self.clock.now()
        if self.wall_limit is not None:
            left = min(left, self.wall_limit - self.clock.wall())
        return left

    def over(self) -> bool:
        """Whether the run was stopped or its time is up, so no step may start."""
        return self.stopped.is_set() or self.left() <= 0

    def walled(self) -> bool:
        """Whether the wall limit has passed, so a step killed now was cut off by it."""
        return self.wall_limit is not None and self.wall_limit - self.clock.wall() <= 0

    def limit(self) -> str:
        """What a step that timed out ran into, for its line in the log."""
        if self.wall_limit is not None and self.walled():
            return f"the wall limit of {self.wall_limit:.0f} s before Enact's kill"
        return f"deadline {self.deadline:.0f} s"

    def run(
        self, cmd: list[str], cwd: Path, env: dict[str, str], cores: int = 0
    ) -> tuple[int | None, str]:
        """The step's exit code, None when the deadline killed it, and its output. The output
        goes to a file, not a pipe, so a killed step's straggler cannot hold up reading it.
        It first waits for up to `cores` CPU tokens (tokens.py), against the deadline.
        """
        lease = tokens.lease(self.klass, cores, self.over)
        if lease is None:
            return None, ""
        with tempfile.TemporaryFile("w+", errors="replace") as out, lease:
            env = {**env, **lease.env()}
            with self.lock:
                if self.over():
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
                while (left := self.left()) > 0:
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
    ) -> tuple[int | None, list[dict[str, object]], str]:
        """Runs `cmd` for `step`, logging its output and how it ended; its exit code, the
        records it logged and the output that names its failed checks."""
        t, wall = clock.now(), clock.wall()
        with run.step(env) as (step_env, results):
            code, output = steps.run(
                cmd, step.cwd, step_env, tokens.step_tokens(lane.name, step.name)
            )
            seconds, wall = clock.now() - t, clock.wall() - wall
            records = run.log(lane.name, step.name, code == 0, wall, results)
        if output.strip():
            log.append(output.rstrip())
        ended = f"{script} {lane.name} {step.name}: "
        if code is None:
            ended += f"TIMED OUT after {took(wall, seconds)} ({steps.limit()})"
        elif code != 0:
            ended += f"FAILED (exit {code}) after {took(wall, seconds)}"
        else:
            ended += took(wall, seconds)
        if code != 0 and step.failed and code not in (None, CANNOT_RUN):
            # Named before the rerun's output follows, so a rerun of the lot says so.
            checks = step.failed(output)
            ended += f"; rerunning {', '.join(checks) if checks else 'it'} once"
            log.append(ended)
            again, rerun, output_again = attempt(
                Step(step.name, step.cmd, step.cwd), cmd + checks
            )
            # A narrowed rerun is a flake only if it ran each check that failed and it passed:
            # a filter that reaches none of them passes having proved nothing.
            missed = (
                unproven(records, rerun, f"{lane.name}:{step.name}") if checks else []
            )
            if again == 0 and missed:
                log.append(
                    f"{ended.split(';')[0]}; the rerun did not pass {', '.join(missed)}:"
                    " the failure stands"
                )
                return code, records, output
            if again == 0:
                flaky.extend(checks or [step.name])
                for r in records:
                    r["flaky"] = r["outcome"] == "fail"
            return again, rerun, output_again
        log.append(ended)
        return code, records, output

    for step in lane.steps:
        t, flakes = clock.now(), len(flaky)
        code, records, output = attempt(step, step.cmd)
        why = verdicts.excused(records) if code not in (0, None, CANNOT_RUN) else []
        if why:
            excused.extend(why)
            held = ", ".join(f"{check} {reason}" for reason, check in why)
            log.append(f"{script} {lane.name} {step.name}: fails no lane: {held}")
            timings.append(f"{step.name} {clock.now() - t:.0f} s, excused")
            continue
        if code != 0:
            timings.append(f"{step.name} {'TIMED OUT' if code is None else 'FAILED'}")
            failure = None
            if step.failed and code not in (None, CANNOT_RUN):
                failure = Failure(step, step.failed(output), records, lane.warm)
            return Outcome(
                lane.name,
                False,
                clock.now() - started,
                "\n".join(log),
                timings,
                flaky,
                excused,
                failure,
                timed_out=code is None,
                walled=code is None and steps.walled(),
            )
        rerun = " after a rerun" if len(flaky) > flakes else ""
        timings.append(f"{step.name} {clock.now() - t:.0f} s{rerun}")
    return Outcome(
        lane.name, True, clock.now() - started, "\n".join(log), timings, flaky, excused
    )


@dataclass
class Report:
    code: int
    outcomes: list[Outcome]
    over: bool
    # Whether the load went over the cores (`Clock.loaded`), or the caches were built cold.
    loaded: bool
    cold: bool
    wall: float


def run_lanes(lanes: list[Lane], script: str, budget: int) -> int:
    return run_all(lanes, script, budget).code


def run_all(
    lanes: list[Lane],
    script: str,
    budget: int,
    wall_limit: float | None = None,
) -> Report:
    """Runs the lanes at once and reports them; with `wall_limit`, a step still running that
    many wall seconds in is killed (`Steps.wall_limit`)."""
    deadline = float(os.environ.get("CHECK_DEADLINE_S", max(2 * budget, HANG_DEADLINE)))
    klass = tokens.script_class(script)
    steps = Steps(deadline, wall_limit=wall_limit, klass=klass)
    cold = bool(warm.ensure()) if any(lane.warm for lane in lanes) else False
    env = dict(os.environ)
    run = runlog.Run.start(script)
    outcomes = play(lanes, run, env, steps)
    # The deadline catches a hang, but it also kills lanes that a rebuild after a rebase or a
    # crowded machine made slow, and a session's answer was to run the whole check again. The
    # lanes it killed rerun once here instead, with a fresh deadline on the same clock: their
    # builds are warm by now, so a slow lane finishes and a hung one hangs again and fails.
    killed = [i for i, o in enumerate(outcomes) if o.timed_out]
    # The wall limit counts from the run's start on the same clock, so a rerun keeps to it.
    if killed and (wall_limit is None or wall_limit - steps.clock.wall() > 0):
        names = {lanes[i].name for i in killed}
        print(
            f"{script}: the deadline killed {', '.join(sorted(names))}; rerunning once with a"
            f" fresh deadline ({load_note()})"
        )
        run.timed_out(names)
        steps = Steps(steps.clock.now() + deadline, steps.clock, wall_limit, klass)
        again = play([lanes[i] for i in killed], run, env, steps)
        for i, o in zip(killed, again):
            first = outcomes[i]
            o.log = f"{first.log}\n--- rerun after the deadline ---\n{o.log}"
            o.timings = [*first.timings, "rerun:", *o.timings]
            outcomes[i] = o
    for o in sorted(outcomes, key=lambda o: not o.ok):
        print(f"=== {script} {o.lane}: {'passed' if o.ok else 'FAILED'}\n{o.log}")
    lines = [f"{o.lane} {o.seconds:.0f} s ({', '.join(o.timings)})" for o in outcomes]
    for line in lines:
        print(f"{script}: {line}")
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
        # A run slowed by other work (a game, `just ladder` on every core) looks like a slow
        # step in every lane at once; the load says which it was before anyone bisects.
        print(f"{script}: {load_note()}")
        if not failed:
            spent = took(steps.clock.wall(), elapsed)
            print(budget_beads.report(script, budget, spent, lines, load_note()))
    code = 1 if failed else 0
    clock = steps.clock
    return Report(code, outcomes, over, clock.loaded(), cold, clock.wall())


def play(
    lanes: list[Lane], run: runlog.Run, env: dict[str, str], steps: Steps
) -> list[Outcome]:
    """Runs the lanes in parallel, each a sequence of steps (`run_lane`)."""
    with ThreadPoolExecutor(max_workers=max(1, len(lanes))) as pool:
        try:
            return list(pool.map(lambda lane: run_lane(lane, run, env, steps), lanes))
        # Ctrl-C reaches only this process, not the steps' sessions.
        except BaseException:
            steps.stop()
            raise


# Lanes.


def repo_lane() -> Lane:
    return (
        Lane("repo")
        .step("just", ["just", "--fmt", "--check", "--unstable"])
        .step(
            "markdown",
            [sys.executable, "-m", "mdformat", "--check", *markdown_files()],
        )
        .step(
            "file-length", [sys.executable, "-m", "beryl_tools.checks", "file-length"]
        )
        .step(
            "conventions", [sys.executable, "-m", "beryl_tools.checks", "conventions"]
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


# The Python tests that read card and vocabulary files, which `scale_lane` runs alone for a Rust
# change (tests/test_scale.py and tests/test_card_names.py read every card file;
# tests/test_card.py reads beryl_card's source as card brief does, and tests/test_specs.py the
# card files' names). Others read files outside tools/ too (test_forge:
# data/card-store.tsv and oracle-divergences.json; test_runlog: git), but never what a card or
# vocabulary change touches.
CARD_READERS = [
    "tests/test_scale.py",
    "tests/test_card_names.py",
    "tests/test_card.py",
    "tests/test_specs.py",
]
CARD_FILES = r"^rust/(cards|crates/beryl_cards?)/"
# The card store is source to both: beryl_card_store embeds it, so a refresh (`just cards`)
# rebuilds every crate that reads a record, and the tools' tests read it and hold it to their
# sample.
CARD_STORE = r"^data/card-store\.tsv$"


def scale_lane() -> Lane:
    """Only the Python tests that read card and vocabulary files: a change to those, which does
    not touch tools/, can break them and slow CI would be the first to say so."""
    return Lane("scale").step(
        "pytest",
        [sys.executable, "-m", "beryl_tools.checks", "pytest", *CARD_READERS],
        rerun=whole,
    )


def rust_lane(fast: bool = False, packages: list[str] | None = None) -> Lane:
    """The rust lane; `fast` leaves out the tests in verdicts.SLOW_ONLY, and `packages` holds
    clippy and the tests to those packages (`affected_packages`)."""
    scope = [a for p in packages for a in ("-p", p)] if packages else []
    named = [f"--packages={','.join(packages)}"] if packages else []
    return (
        Lane("rust", warm=True)
        .step(
            "build-paths", [sys.executable, "-m", "beryl_tools.checks", "build-paths"]
        )
        .step(
            "fmt",
            [sys.executable, "-m", "beryl_tools.checks", "rustfmt", "--check"],
            paths.RUST,
        )
        .step(
            "clippy",
            [
                "cargo",
                "clippy",
                "--locked",
                *scope,
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            paths.RUST,
        )
        .step(
            "tests",
            [
                sys.executable,
                "-m",
                "beryl_tools.checks",
                "fast-rust-tests" if fast else "rust-tests",
                *named,
            ],
            rerun=failed_tests,
        )
        # Every card file's rendered rules text against its Oracle text (card_text.py), once
        # the tests have built the scenario binary.
        .step("card-text", [sys.executable, "-m", "beryl_tools.checks", "card-text"])
    )


def ui_lane() -> Lane:
    return (
        Lane("ui", warm=True)
        .step("prettier", [system.npm_bin("prettier"), "--check", "."], paths.UI)
        .step("tsc", [system.npm_bin("tsc"), "-b"], paths.UI)
        # The UI's logic that runs with no browser (ui/package.json `test`): Node expands the
        # pattern itself.
        .step("test", ["node", "--test", "scripts/*.test.mjs"], paths.UI)
    )


def names_lane(
    cited: list[str] | None = None, fetch: bool = False, crates: bool | None = None
) -> Lane:
    """No card name in generic code, held to the burn-down list (card_names.py), no other
    player of two, no board door and no failed push thrown away in rules code (two_player.py,
    board_door.py, discarded_push.py), no citation of a planning document in code
    (plan_references.py), and every rules citation held to the pinned release
    (citations.py) and card file header to the card store (card_header.py): in the files
    `cited`, or with None in the whole tree, fetching the release's text if `fetch` and it is
    not cached. With `crates` (by default when `cited` is None), the set crates are held to
    what `just card crates` writes (card_crates.py).
    """
    checks = [sys.executable, "-m", "beryl_tools.checks"]
    lane = Lane("names")
    for check in (
        "card-names",
        "two-player",
        "board-door",
        "discarded-push",
        "plan-references",
    ):
        lane.step(check, [*checks, check])
    if cited != []:
        flags = ["--fetch"] if fetch else []
        lane.step("citations", [*checks, "citations", *flags, *(cited or [])])
        headers = [p for p in cited or [] if card_files.FILE.match(p)]
        if cited is None or headers:
            lane.step("card-headers", [*checks, "card-headers", *headers])
    if crates or (crates is None and cited is None):
        lane.step("card-crates", [*checks, "card-crates"])
    return lane


def app_lane() -> Lane:
    return Lane("app", warm=True).step("smoke", [*BY, "smoke"], rerun=whole)


def shots_lane(select: bool = False) -> Lane:
    """Renders every screen in ui/shots.json headless, or with `select` those the change can
    break (selection.py); a page error or a crash fails it."""
    cmd = [*BY, "ui-shots", *(["--select"] if select else [])]
    return Lane("shots", warm=True).step("ui-shots", cmd, rerun=failed_shots)


def ui_test_lane(select: bool = False) -> Lane:
    """Plays the game screen through fixtures it records apart from `shots`' (shots.py), so
    the two run at once, every check or with `select` those the change can break; a page
    error or a failed interaction fails it."""
    cmd = [*BY, "ui-test", *(["--select"] if select else [])]
    return Lane("ui-test", warm=True).step("ui-test", cmd, rerun=failed_titles)


def oracle_lane() -> Lane:
    """Plays in XMage and Forge the scenarios of the cards changed since master and a sample,
    or of every card when the exporters or harness changed (`by oracle --select`,
    oracle_selection.py; the nightly plays them all): a failure the divergence allowlist does
    not explain fails it, as does an oracle not built at its pin."""
    cmd = [*BY, "oracle", "--select"]
    return Lane("oracle", warm=True).step("oracle", cmd, rerun=failed_cards)


# How long the fast check's oracle lane waits for another oracle run, such as slow CI's, before
# it skips: that run holds the oracles for up to a minute and a half.
ORACLE_LOCK_WAIT = 10


def card_oracle_lane(cards: list[str]) -> Lane:
    """Plays the scenarios of `cards` in XMage and Forge without Forge's AI games, skipping
    them, and passing, when another oracle run holds the oracles past the wait."""
    cmd = [*BY, "oracle", "--no-sim", "--lock-wait", str(ORACLE_LOCK_WAIT), *cards]
    return Lane("oracle", warm=True).step("oracle", cmd, rerun=failed_cards)


def fast(args: argparse.Namespace) -> int:
    started = speed.start_background()
    if started:
        print(f"check: {started}")
    base = branch_base()
    changed: list[str] | None = changed_paths(base)
    problems = hygiene.problems(base, changed or [])
    for p in problems:
        print(f"check: {p}")
    if problems:
        return 1
    print(
        f"check: {'every lane (no base)' if changed is None else f'{len(changed)} paths changed since {base[:8]}'}"
    )
    lanes = narrow(fast_lanes(changed), base, fast_lanes, lambda l: l.name)
    return run_lanes(lanes, "check", FAST_BUDGET)


def fast_lanes(changed: list[str] | None) -> list[Lane]:
    """The fast check's lanes for the paths changed since the branch base; None: every lane."""
    every: bool = changed is None

    def touched(pattern: str) -> bool:
        return every or any(re.match(pattern, p) for p in changed or [])

    lanes = [repo_lane()]
    if touched(r"^tools/") or touched(CARD_STORE):
        lanes.append(python_lane())
    elif touched(CARD_FILES):
        lanes.append(scale_lane())  # The python lane runs these tests whole.
    if touched(r"^rust/") or touched(CARD_STORE):
        lanes.append(rust_lane(True, affected_packages(changed)))
    if touched(script_checks.inputs()) or touched(CARD_STORE):
        lanes.append(script_checks.lane())
    if touched(r"^ui/"):
        lanes.append(ui_lane())
    # The changed files that may cite the rules, held to the cached release's text.
    cited = None if every else [p for p in changed or [] if citations.cites(p)]
    # The set crates as `just card crates` writes them: a card file, a store refresh, a new set
    # group or a keyword added to the engine can call for a new one.
    inputs = re.escape(store_cards.KEYWORDS_SOURCE_REL)
    crates = every or touched(
        rf"^rust/cards/|^data/(card-store\.tsv|set-groups\.json)$|^{inputs}$"
    )
    if (
        cited
        or crates
        or touched(r"^(tools|rust|ui)/|^data/card-names-allowlist\.json$")
    ):
        lanes.append(names_lane(cited, crates=crates))
    # The browser checks the change can break, as many as fit the budget; none on
    # Windows (WINDOWS_SKIPS).
    if not every and touched(r"^(rust|ui)/") and not system.WINDOWS:
        lanes += [ui_test_lane(select=True), shots_lane(select=True)]
    from . import motion  # It imports this module, by way of shots.py.

    if not every and touched(motion.INPUTS) and not system.WINDOWS:
        lanes.append(motion.lane())
    if cards := changed_cards(changed):
        lanes.append(card_oracle_lane(cards))
    return lanes


def changed_cards(changed: list[str] | None) -> list[str]:
    """The cards whose files changed, for the oracle lane: none with no base, when every card
    would count (slow CI checks them all), or on a machine without the oracles built
    (`just oracle-setup`)."""
    from . import oracles  # It imports this module.

    if not changed or not all(oracles.built(o) for o in oracles.ORACLES):
        return []
    return card_files.cards_in(changed)


def slow(args: argparse.Namespace) -> int:
    measuring = speed.start_background()
    if measuring:
        print(f"ci: {measuring}")
    started = time.monotonic()
    lanes = {
        "repo": repo_lane(),
        "python": python_lane(),
        "rust": rust_lane(),
        "script": script_checks.lane(),
        "ui": ui_lane(),
        "names": names_lane(fetch=True),
        "app": app_lane(),
        "shots": shots_lane(),
        "ui-test": ui_test_lane(),
        "oracle": oracle_lane(),
        "counts": bench.lane(),
        "mobile": mobile.lane(),
    }
    if system.WINDOWS and not args.lanes:
        print(
            f"ci: skipping {' '.join(WINDOWS_SKIPS)} on Windows; the Mac's slow CI runs them"
        )
        for name in WINDOWS_SKIPS:
            del lanes[name]
    chosen = args.lanes or list(lanes)
    unknown = set(chosen) - set(lanes)
    if unknown:
        raise SystemExit(f"unknown lanes {sorted(unknown)}; lanes: {' '.join(lanes)}")
    budget = int(os.environ.get("CI_BUDGET_S", SLOW_BUDGET))
    return slow_run([lanes[n] for n in chosen], budget, started)


def slow_run(lanes: list[Lane], budget: int, started: float) -> int:
    """Slow CI on `lanes`, begun at `started` (time.monotonic). A failure that survived its
    rerun is bisected (bisect.py); a run over budget fails nothing (budget.py)."""
    from . import bisect  # It imports this module.

    report = run_all(lanes, "ci", budget, wall_limit=bisect.time_left(started))
    failed = [o for o in report.outcomes if not o.ok]
    if failed:
        bisect.after(failed, started)
    return report.code


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser("check", help="the fast check (enact push)").set_defaults(func=fast)
    p = sub.add_parser("ci", help="slow CI, or some of its lanes")
    p.add_argument("lanes", nargs="*")
    p.set_defaults(func=slow)


if __name__ == "__main__":
    steps: dict[str, Callable[..., int]] = {
        "pytest": python_tests.pytest_step,
        "rust-tests": rust_tests_step,
        "fast-rust-tests": fast_rust_tests_step,
        "card-names": card_names.step,
        "card-text": card_text.step,
        "card-headers": card_header.step,
        "card-crates": card_crates.step,
        "file-length": file_length.step,
        "two-player": two_player.step,
        "board-door": board_door.step,
        "plan-references": plan_references.step,
        "discarded-push": discarded_push.step,
        "citations": citations.step,
        "conventions": conventions.step,
        "build-paths": build_paths.step,
        "rustfmt": card_files.rustfmt_step,
    }
    sys.exit(steps[sys.argv[1]](*sys.argv[2:]))
