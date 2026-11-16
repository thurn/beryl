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

Every full run, the sweep's included, writes each test's time to the run log from nextest's
JUnit report, as `b3d-test:<binary>::<test>`. Plans read the times from the fast check's lane
and the sweep's, so a test the fast check keeps cutting is still timed by the sweep.
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
import xml.etree.ElementTree as ET
from datetime import datetime, timedelta, timezone
from pathlib import Path

from .. import deadline, runlog, runstore, rust_fit, tokens
from ..rust_tests import NEXTEST_PROFILE, junit_results, nextest_filter
from ..selection import Record
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


def run(records: Known | None) -> int:
    """Runs the workspace's tests, those that fit when `records` are given, in a lease of CPU
    tokens whose size is the threads the plan counts on, and logs each test's result."""
    lease = tokens.acquire(
        tokens.named_class(), tokens.largest(tokens.pool_size()), command="nextest"
    )
    assert lease is not None
    with lease:
        env = {**os.environ, **lease.env()}
        expression = "all()"
        limit = deadline.until_hard_limit()
        if records is not None and limit is not None:
            tests = rust_fit.listed(["--workspace"], "all()", paths.WORKSPACE, env)
            # rust_fit.plan keeps back its own margin as well.
            left = limit - MARGIN + rust_fit.MARGIN
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
        JUNIT.unlink(missing_ok=True)
        code = subprocess.run(
            [
                *["cargo", "nextest", "run", f"--profile={NEXTEST_PROFILE}"],
                *["--workspace", "--cargo-quiet", "--no-fail-fast", "--no-tests=pass"],
                *["--status-level=fail", "--final-status-level=fail"],
                *["-E", expression],
            ],
            cwd=paths.WORKSPACE,
            env=env,
            check=False,
        ).returncode
    try:
        for check, outcome, took in junit_results(JUNIT.read_text()):
            runlog.result(check_of(check), outcome, took)
    except (OSError, ET.ParseError) as e:
        print(f"cargo-test: no per-test results for the run log: {e}", flush=True)
    return code
