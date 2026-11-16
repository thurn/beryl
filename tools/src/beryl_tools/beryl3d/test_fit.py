"""How Beryl 3D's `cargo-test` step runs the workspace's tests under nextest: every test, each
one's outcome and time written to the run log, and inside a fast check only the tests that
finish before the fast check's hard limit.

The step used to run every test of the workspace however little time the run had left, and
logged no test by name. Between 16:10 and 18:10 UTC on 2026-11-16, it started in 30 fast
checks. It took 40-130 s when it finished, and the 180 s hard limit cut it off in 16 of them,
which made `beryl3d-tests` the last lane to finish in 29 of 47 fast checks. A step cut off at
the limit reports nothing, so the push waited the full 180 s and the CPU the tests spent was
thrown away.

Now, inside a fast check, the step plans as the Rust lane's tests step does (rust_fit.py): it
lists the tests the shared build made (`cli.SHARED_BUILD` builds every test target) and runs
those that fit, in rust_fit's order (untimed tests first, then those that caught a break, then
those that fail most, then the cheapest). The tests it cuts are logged as skipped and named in
the output as going to the Beryl 3D sweep, which runs every test on staging (sweep.py).

It plans against the time left until the hard limit ([`deadline.until_hard_limit`]), less
[`MARGIN`], not against the fast check's 60-90 s target as the Rust lane does. The step starts
after the shared build and the recordings (`record`, 40-180 s in those fast checks), so the
target had nearly always passed by then and every test would have been cut. The other Beryl 3D
lanes run past the target anyway (`test` and `timeline-parity` do not plan), so tests that end
before the limit cost a push no extra wait, and they caught two breaks on staging the first time
the step ran to its end.

A test that failed in the last [`RECENT`] on the same branch, or in the sweep on staging, is
never cut: it runs first, and the rest fit the time left. On 2026-11-16 a commit's fast check
failed two Beryl 3D tests, its retry cut them for time, and the commit landed and left staging's
Beryl 3D tests failing for 72 minutes. A test the sweep saw fail on staging is kept as well, so a
branch based on a broken staging sees the failure before it pushes on top of it.

The plan's times are estimates, measured at other loads, so a plan can still run past the limit,
and a step the limit kills reports nothing: after test_fit landed the step was still cut off in 4
of 5 fast checks between 21:00 and 21:20 UTC on 2026-11-16, twice after cutting tests from its
plan. So the step plans against the time left after nextest has listed the tests, which runs
every test binary once, and it stops nextest itself [`STOP`] seconds before the limit. nextest
writes its JUnit report when it is stopped, so the tests that finished are logged as usual, and
those it stopped or never started are logged as skipped and named as deferred to the sweep, as a
cut test is.

Every full run, the sweep's included, writes each test's time to the run log from nextest's
JUnit report, as `b3d-test:<binary>::<test>`. Plans read the times from the fast check's lane
and the sweep's, so a test the fast check keeps cutting is still timed by the sweep.

nextest fixes its thread count when it starts, so a step whose lease got few of the tokens it
asked for kept that many threads to the end. Between 19:00 and 22:10 UTC on 2026-11-16, 20 of 45
fast-check nextest leases got 1 of the 12 tokens they wanted, at once rather than after a wait,
and those runs took 60-115 s against 18-25 s for runs that got 12. The pool's tokens free up
within tens of seconds (sampled every 2 s, 18 held fell to 4 in 30 s), and the run kept none of
them. So inside a fast check, a step whose lease is short runs its tests in waves of about
[`WAVE`] seconds each, takes the tokens free before each wave, and runs everything left in one
wave once the lease is full ([`run_waves`]).
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import threading
import xml.etree.ElementTree as ET
from datetime import datetime, timedelta, timezone
from pathlib import Path

from .. import deadline, runlog, runstore, rust_fit, tokens
from ..rust_tests import NEXTEST_PROFILE, junit_results, nextest_filter
from ..selection import DEFAULT_SECONDS, Record
from . import paths

# The run-log lanes the step runs in: the fast check's (lane.GROUPS) and the sweep's (each of
# its steps is a lane named `sweep.LANE_PREFIX` and the step).
FAST_LANE = "beryl3d-tests"
SWEEP_LANE = "b3d-cargo-test"
LANES: tuple[str, str] = (FAST_LANE, SWEEP_LANE)
# The prefix of each test's check, which keeps them apart from the main workspace's `rust:`.
PREFIX = "b3d-test"
# Where nextest's `check` profile writes its JUnit report (rust/beryl3d/.config/nextest.toml).
JUNIT: Path = paths.WORKSPACE / "target" / "nextest" / NEXTEST_PROFILE / "junit.xml"
# The seconds kept back before the hard limit: for nextest to start, for slow tests the
# records underestimate, and for the step to write its results.
MARGIN = 20.0
# The seconds before the hard limit at which the step stops nextest: enough for nextest to stop
# its tests and write its report, and for the step to log the results and exit.
STOP = 8.0
# The seconds of tests, by their records, that one wave runs on a short lease's threads
# ([`run_waves`]). Starting nextest again inside a lease takes about 0.5 s once the tests are
# built, so a wave this long spends little on starting and still takes freed tokens often.
WAVE = 8.0
# How far back a test's failure keeps it from being cut. A branch's retries and the sweeps of
# the staging it is based on fall within a few hours, and each hour of the run log is about
# 100 MB to scan.
RECENT = timedelta(hours=3)
# The bytes every failed test's record holds (runlog.Run.records writes them this way), so the
# scan parses only the lines that have them.
FAILED = b'"outcome": "fail"'
TEST: bytes = f'"check": "{PREFIX}:'.encode()


def recent_failures(place: str, since: datetime) -> set[str]:
    """The tests that failed from `since` on in the fast check on the branch `place`
    (runlog.place) or in the sweep. A failure of a step cut off or killed (`timeout`) says
    nothing about a test and is left out."""
    found: set[str] = set()
    for line in runstore.read_lines(since):
        if FAILED not in line or TEST not in line:
            continue
        try:
            r = json.loads(line)
            when = datetime.fromisoformat(r["time"])
        except (KeyError, TypeError, ValueError):
            continue
        if when < since or r.get("outcome") != "fail" or r.get("timeout"):
            continue
        lane = r.get("lane")
        if lane == SWEEP_LANE or (lane == FAST_LANE and r.get("place") == place):
            found.add(str(r["check"]))
    return found


class Known:
    """The records of the step's tests in both its lanes (deadline.Records), and the tests
    that failed recently ([`recent_failures`]), read in threads while the step builds. Where
    both lanes ran a test, the record with more runs counts.
    """

    def __init__(self) -> None:
        self.lanes: list[deadline.Records] = [deadline.Records(lane) for lane in LANES]
        self.failed: set[str] = set()
        self.thread = threading.Thread(target=self.read_failures, daemon=True)
        self.thread.start()

    def read_failures(self) -> None:
        since = datetime.now(timezone.utc) - RECENT
        try:
            self.failed = recent_failures(runlog.place("check"), since)
        except Exception as e:  # A plan without them still fits; it may cut them.
            print(f"cargo-test: no recent failures to keep: {e}", flush=True)

    def failures(self) -> set[str]:
        self.thread.join()
        return self.failed

    def get(self) -> dict[str, Record]:
        merged: dict[str, Record] = {}
        for lane in self.lanes:
            for check, record in lane.get().items():
                if check not in merged or record.runs > merged[check].runs:
                    merged[check] = record
        return merged


def known() -> Known | None:
    """The records to plan with inside a fast check; None outside one, where every test runs."""
    return Known() if deadline.started() is not None else None


def check_of(junit_check: str) -> str:
    """The run-log check of a test named by rust_tests.junit_results (`rust:<binary>::<test>`)."""
    return PREFIX + junit_check.removeprefix("rust")


def deferred_line(cut: list[str], left: float) -> str:
    return (
        f"cargo-test: {len(cut)} tests deferred to the Beryl 3D sweep: they do not fit the"
        f" {max(left, 0):.0f} s left before the fast check's hard limit"
    )


def kept_line(kept: list[str]) -> str:
    return (
        f"cargo-test: {len(kept)} tests that failed in the last"
        f" {RECENT.total_seconds() / 3600:.0f} h run first and are never cut: "
        + ", ".join(kept)
    )


def stopped_line(stopped: list[str]) -> str:
    return (
        f"cargo-test: {len(stopped)} tests deferred to the Beryl 3D sweep: stopped or not"
        f" started {STOP:.0f} s before the fast check's hard limit"
    )


def nextest(
    expression: str, env: dict[str, str], stop: float | None
) -> tuple[int, bool]:
    """Runs the workspace's tests that match `expression` under nextest; its exit code, and
    whether it was stopped because `stop` seconds passed first. nextest stops its tests on
    SIGTERM and still writes its JUnit report. The cargo shim and cargo both replace themselves
    with the next program, so the process started here is nextest itself."""
    child = subprocess.Popen(
        [
            *["cargo", "nextest", "run", f"--profile={NEXTEST_PROFILE}"],
            *["--workspace", "--cargo-quiet", "--no-fail-fast", "--no-tests=pass"],
            *["--status-level=fail", "--final-status-level=fail"],
            *["-E", expression],
        ],
        cwd=paths.WORKSPACE,
        env=env,
    )
    try:
        return child.wait(timeout=None if stop is None else max(stop, 0.0)), False
    except subprocess.TimeoutExpired:
        child.send_signal(signal.SIGTERM)
        try:
            return child.wait(timeout=STOP / 2), True
        except subprocess.TimeoutExpired:
            child.kill()
            return child.wait(), True


def run(records: Known | None) -> int:
    """Runs the workspace's tests, those that fit when `records` are given, in a lease of CPU
    tokens whose size is the threads the plan counts on, and logs each test's result. Inside a
    fast check it stops the tests [`STOP`] seconds before the hard limit."""
    lease = tokens.acquire(
        tokens.named_class(), tokens.largest(tokens.pool_size()), command="nextest"
    )
    assert lease is not None
    chosen: list[str] | None = None
    with lease:
        env = {**os.environ, **lease.env()}
        expression = "all()"
        tests: list[str] | None = None
        if records is not None and deadline.until_hard_limit() is not None:
            tests = rust_fit.listed(["--workspace"], "all()", paths.WORKSPACE, env)
            # Read after the listing, which runs every test binary and may take a while
            # under load. rust_fit.plan keeps back its own margin as well.
            left = (deadline.until_hard_limit() or 0.0) - MARGIN + rust_fit.MARGIN
            if tests is not None:
                failed = records.failures()
                chosen, _, cut = rust_fit.plan(
                    tests, [], records.get(), left, lease.size, PREFIX, failed
                )
                kept = sorted(
                    c for t in chosen if (c := rust_fit.check_id(t, PREFIX)) in failed
                )
                if kept:
                    print(kept_line(kept), flush=True)
                if cut:
                    print(deferred_line(cut, left - rust_fit.MARGIN), flush=True)
                    for check in cut:
                        runlog.result(check, "skip", 0.0, "cut")
                    if not chosen:
                        return 0
                    expression = nextest_filter(chosen)
        planned = chosen if chosen is not None else tests
        if records is not None and planned and short(lease):
            return run_waves(lease, planned, records.get())
        JUNIT.unlink(missing_ok=True)
        limit = deadline.until_hard_limit()
        code, stopped = nextest(
            expression, env, None if limit is None else limit - STOP
        )
    try:
        results = junit_results(JUNIT.read_text())
    except (OSError, ET.ParseError) as e:
        print(f"cargo-test: no per-test results for the run log: {e}", flush=True)
        results = []
    if not stopped:
        for check, outcome, took in results:
            runlog.result(check_of(check), outcome, took)
        return code
    return log_stopped(results, chosen)


def short(lease: tokens.Lease) -> bool:
    """Whether `lease` owns tokens of a class and holds fewer than it wanted, so that the tests
    run in waves that take the tokens that free up ([`run_waves`])."""
    return bool(lease.files) and bool(lease.klass) and lease.size < lease.wanted


def read_results() -> list[tuple[str, str, float]]:
    try:
        return junit_results(JUNIT.read_text())
    except (OSError, ET.ParseError) as e:
        print(f"cargo-test: no per-test results for the run log: {e}", flush=True)
        return []


def next_wave(
    order: list[str], records: dict[str, Record], threads: int, full: bool
) -> list[str]:
    """The tests from the front of `order` that make up the next wave: all of them once the
    lease is `full`, and otherwise those whose recorded seconds fill about [`WAVE`] seconds on
    `threads` threads, at least one."""
    if full:
        return order
    wave: list[str] = []
    spent = 0.0
    for test in order:
        if wave and spent / max(1, threads) >= WAVE:
            break
        wave.append(test)
        spent += seconds_of(test, records)
    return wave


def seconds_of(test: str, records: dict[str, Record]) -> float:
    record = records.get(rust_fit.check_id(test, PREFIX))
    return record.seconds if record is not None else DEFAULT_SECONDS


def run_waves(lease: tokens.Lease, tests: list[str], records: dict[str, Record]) -> int:
    """Runs `tests` in waves, growing `lease` before each wave by the tokens free then, and
    logs every test's result. Tests run longest first, so the long ones are not left for the
    end. While the lease is short it counts as a push's waiter, as a growing program's does
    (tokens.run_growing), so lower classes take no tokens and their elastic builds give theirs
    back. Once it is full, the rest run in one last wave. A wave stopped [`STOP`] seconds before
    the hard limit logs the tests that finished, in every wave, and defers the rest."""
    pool = tokens.Pool()
    me = pool.waiter(lease.klass)
    order = sorted(tests, key=lambda t: -seconds_of(t, records))
    results: list[tuple[str, str, float]] = []
    code = 0
    sizes: list[int] = []
    try:
        while order:
            if short(lease):
                if lease.klass in tokens.GROW_AS_WAITER:
                    me.write_text(f"{os.getpid()} {lease.klass}")
                lease.grow(pool.top_up(lease.klass, lease.size, lease.wanted))
            full = not short(lease)
            if full:
                me.unlink(missing_ok=True)
            wave = next_wave(order, records, lease.size, full)
            order = order[len(wave) :]
            sizes.append(lease.size)
            JUNIT.unlink(missing_ok=True)
            limit = deadline.until_hard_limit()
            wave_code, stopped = nextest(
                nextest_filter(wave),
                {**os.environ, **lease.env()},
                None if limit is None else limit - STOP,
            )
            if stopped:
                print(waves_line(sizes), flush=True)
                return log_stopped(results + read_results(), tests)
            results += read_results()
            code = code or wave_code
    finally:
        me.unlink(missing_ok=True)
    print(waves_line(sizes), flush=True)
    for check, outcome, took in results:
        runlog.result(check_of(check), outcome, took)
    return code


def waves_line(sizes: list[int]) -> str:
    return (
        f"cargo-test: {len(sizes)} waves on " + ", ".join(map(str, sizes)) + " threads"
    )


def log_stopped(results: list[tuple[str, str, float]], chosen: list[str] | None) -> int:
    """Logs the `results` of a run stopped at [`STOP`]: a test that failed by itself fails, a
    test that the stop aborted is skipped, as is each of the `chosen` tests (every test when
    None) that never started. A test aborted by the stop shows in the report as a failure with
    the signal. The step fails only for a test that failed by itself."""
    aborted = signal_failures(JUNIT)
    logged: set[str] = set()
    stopped: list[str] = []
    failed = False
    for check, outcome, took in results:
        name = check_of(check)
        logged.add(name)
        if name in aborted:
            stopped.append(name)
            runlog.result(name, "skip", 0.0, "cut")
            continue
        failed = failed or outcome == "fail"
        runlog.result(name, outcome, took)
    for test in chosen or []:
        name = rust_fit.check_id(test, PREFIX)
        if name not in logged:
            stopped.append(name)
            runlog.result(name, "skip", 0.0, "cut")
    print(stopped_line(stopped), flush=True)
    return 1 if failed else 0


def signal_failures(junit: Path) -> set[str]:
    """The run-log checks of the tests in nextest's JUnit report that failed because a signal
    stopped them: nextest's failure says `process aborted with signal`."""
    try:
        root = ET.fromstring(junit.read_text())
    except (OSError, ET.ParseError):
        return set()
    found: set[str] = set()
    for case in root.iter("testcase"):
        failure = case.find("failure")
        if failure is not None and "aborted with signal" in (
            failure.get("message") or ""
        ):
            found.add(f"{PREFIX}:{case.get('classname')}::{case.get('name')}")
    return found
