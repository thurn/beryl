"""Measure the verified checkout for the frame trend exploration family."""

from __future__ import annotations

import subprocess
import sys

from .. import calibration
from ..client import counts, frame_profiles, gate, trend
from . import Result
from .client import pack_path
from .client_frame_counts import play
from .rust import asked

FAMILY = "client-frame-trend"
MESSAGE_LIMIT = 1500


def next_checks() -> list[dict[str, str]]:
    """Ignite runs in the verified checkout. Offer its HEAD once per machine's series."""
    commit = trend.current_commit()
    check = f"{FAMILY}:{commit}"
    try:
        provenance = trend.current_provenance()
    except calibration.SetupError as error:
        print(f"{FAMILY} setup: {error}", file=sys.stderr)
        return [{"check": check, "digest": commit}]
    if trend.through_commit(trend.load(trend.series_path(provenance)), commit):
        return []
    return [{"check": check, "digest": f"{commit}:{provenance.digest}"}]


def collect() -> trend.Boards:
    boards, enhanced = play()
    native = gate.report(boards, enhanced)

    # Ignite has already built the gate and its pack. Use them directly and take the
    # benchmark VM only when its own lock permits an optional run.
    instructions = {}
    try:
        profile = frame_profiles.registered("instructions")
        found = counts.collect(boards, profile, wait=False, pack=pack_path())
        instructions = {board: counts.counters(value) for board, value in found.items()}
    except calibration.SetupError as error:
        print(f"{FAMILY}: {error}", file=sys.stderr)
    return trend.with_instructions(native, instructions)


def run(checks: list[str]) -> list[Result]:
    wanted = asked(FAMILY, checks)
    if not wanted:
        return []
    head = trend.current_commit()
    try:
        provenance = trend.current_provenance()
    except calibration.SetupError as error:
        return [
            Result(f"{FAMILY}:{commit}", "incomplete", str(error)) for commit in wanted
        ]
    path = trend.series_path(provenance)
    results: list[Result] = []
    for commit in wanted:
        check = f"{FAMILY}:{commit}"
        if commit != head:
            # Ignite can resume a saved batch after the verified tip moved. Retire the
            # old check without attributing this checkout's work to that commit.
            results.append(Result(check, "incomplete", f"Checkout is now at {head}."))
            continue
        try:
            history = trend.record(path, commit, collect, provenance, reuse=True)
            lines = trend.regressions(history, gate.read_budgets()["trend"])
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            # Without a measurement, leave the batch unreported so Ignite can retry it.
            raise SystemExit(f"{check}: {error}") from error
        message = "\n".join(lines)
        if len(message) > MESSAGE_LIMIT:
            message = message[: MESSAGE_LIMIT - 1] + "…"
        results.append(Result(check, "fail" if lines else "pass", message))
    return results
