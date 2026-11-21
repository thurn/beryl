"""Measure the verified checkout for the frame trend exploration family."""

from __future__ import annotations

import subprocess

from ..beryl3d import gate, trend
from . import Result
from .beryl3d import DEBUG, pack_path, run_out
from .rust import asked

FAMILY = "b3d-frame-trend"
MESSAGE_LIMIT = 1500


def next_checks() -> list[dict[str, str]]:
    """Ignite runs in the verified checkout. Offer its HEAD once per machine's series."""
    commit = trend.current_commit()
    if trend.through_commit(trend.load(trend.series_path()), commit):
        return []
    return [{"check": f"{FAMILY}:{commit}", "digest": commit}]


def collect() -> trend.Boards:
    done = run_out([str(DEBUG / "gate")])
    if done.returncode:
        raise ValueError(done.stderr.strip() or f"gate exited {done.returncode}")
    native = gate.report(gate.parse(done.stdout), gate.parse_enhanced(done.stdout))

    # Ignite has already built the gate and its pack. Use them directly and take the
    # benchmark VM only when its own lock permits an optional run.
    from ..beryl3d import counts

    output = counts.measure(list(native), wait=False, pack=pack_path())
    instructions = (
        {board: counts.counters(found) for board, found in counts.parse(output).items()}
        if output is not None
        else {}
    )
    return trend.with_instructions(native, instructions)


def run(checks: list[str]) -> list[Result]:
    wanted = asked(FAMILY, checks)
    if not wanted:
        return []
    head = trend.current_commit()
    path = trend.series_path()
    results: list[Result] = []
    for commit in wanted:
        check = f"{FAMILY}:{commit}"
        if commit != head:
            # Ignite can resume a saved batch after the verified tip moved. Retire the
            # old check without attributing this checkout's work to that commit.
            results.append(Result(check, "incomplete", f"Checkout is now at {head}."))
            continue
        try:
            history = trend.record(path, commit, collect, reuse=True)
            lines = trend.regressions(history, gate.read_budgets()["trend"])
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            # Without a measurement, leave the batch unreported so Ignite can retry it.
            raise SystemExit(f"{check}: {error}") from error
        message = "\n".join(lines)
        if len(message) > MESSAGE_LIMIT:
            message = message[: MESSAGE_LIMIT - 1] + "…"
        results.append(Result(check, "fail" if lines else "pass", message))
    return results
