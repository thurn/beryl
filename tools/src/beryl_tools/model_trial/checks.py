"""The check verdict on a model trial arm's final commit, and `model-trial baseline`, which
records how checks ended at a pair's start commit for it."""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from . import io, manifest
from .spec import Spec

SHA: re.Pattern[str] = re.compile(r"[0-9a-f]{7,64}")


@dataclass
class CheckVerdict:
    verdict: str  # pass, fail or unknown
    # The failed checks that failed the same way at the start commit, and the others.
    preexisting: list[str] = field(default_factory=list)
    failures: list[str] = field(default_factory=list)
    # Failed checks whose outcome at the start commit is not recorded yet.
    unbaselined: list[str] = field(default_factory=list)


def check_verdict(
    commit: str | None,
    landed: bool,
    session: str = "",
    start_commit: str | None = None,
    baselines: dict[tuple[str, str], manifest.Baseline] | None = None,
) -> CheckVerdict:
    """The check verdict on an arm's final `commit`.

    `ignite land` refuses a change without a passing gate verdict, so a landed source is `pass`.
    For a copy, which closes without landing, the verdict is first read from the Ignite daemon's
    stored gate verdicts by the tree hash of `commit`. Copies are pinned to the machine that
    grades them, so the daemon there saw their checks.

    The daemon stores passes only, and a copy starts from a frozen `staging` commit that may
    already fail checks. The trial's first copies started from a commit where
    `source:file-length` and three pytest tests failed, so no copy from it could ever store a
    pass, and the grader marked them all `fail`. So a copy with no stored pass is judged by the
    failures of the last `ignite check` its `session` ran after `commit` was made: a failure
    that failed with the same message at `start_commit` (a `Baseline` row) is not the copy's.
    The copy is `pass` when every failure is like that, `fail` when one is not, and `unknown`
    when a failure has no baseline yet. A copy that never ran a check after its last commit is
    `fail`. `unknown` also means no commit, or a database that cannot be read."""
    if landed:
        return CheckVerdict("pass")
    if not commit or not SHA.fullmatch(commit):
        return CheckVerdict("unknown")
    try:
        tree = io.git("rev-parse", f"{commit}^{{tree}}")
    except io.IgniteError:
        return CheckVerdict("unknown")
    passed = io.gate_passed(tree) if SHA.fullmatch(tree) else None
    if passed is None:
        return CheckVerdict("unknown")
    if passed:
        return CheckVerdict("pass")
    made = io.git("log", "-1", "--format=%cI", commit) if session else ""
    failed = io.check_failures(session, utc(made)) if made else None
    if not failed or not start_commit:
        return CheckVerdict("fail")
    found = CheckVerdict("pass")
    for name, message in sorted(failed.items()):
        base = (baselines or {}).get((start_commit, name))
        if base is None:
            found.unbaselined.append(name)
        elif base.outcome == "fail" and same_failure(base.message, message):
            found.preexisting.append(name)
        else:
            found.failures.append(name)
    found.verdict = (
        "fail" if found.failures else "unknown" if found.unbaselined else "pass"
    )
    return found


def same_failure(at_start: str, message: str) -> bool:
    """Whether a copy's failure `message` is the failure its start commit had, `at_start`.

    Only the first line is compared: it holds the fault (the file over the limit, the count
    against its bar, the assertion), while later lines carry run details such as how many
    other tests failed beside it. A copy whose session only ever saw "failed before on these
    inputs" for the check never failed it on inputs of its own: the daemon matched the inputs
    to a failure that another session recorded, which for inputs a copy did not change is the
    start commit's failure."""
    if message == io.FAILED_BEFORE:
        return True
    return at_start.split("\n", 1)[0] == message.split("\n", 1)[0]


def utc(at: str) -> str:
    """An ISO time with an offset as the daemon writes times: UTC, ending in `Z`."""
    moved = datetime.fromisoformat(at.replace("Z", "+00:00")).astimezone(timezone.utc)
    return moved.strftime("%Y-%m-%dT%H:%M:%S.") + f"{moved.microsecond // 1000:03d}Z"


def baselines_from(
    trial: str, start: str, reply: dict[str, Any]
) -> list[manifest.Baseline]:
    """The baselines an `ignite test --json` `reply` run at `start` gives: every check it ran,
    with the failure message of each that failed."""
    messages = {
        str(f.get("check")): str(f.get("message") or "")
        for f in reply.get("failures") or []
    }
    return [
        manifest.Baseline(
            trial=trial,
            start_commit=start,
            check=str(o["check"]),
            outcome=str(o["outcome"]),
            message=messages.get(str(o["check"]), ""),
        )
        for o in reply.get("outcomes") or []
    ]


def cmd_baseline(args: argparse.Namespace, spec: Spec) -> int:
    """Records how checks ended at a pair's start commit, from the `ignite test --json` reply in
    `--results`, which was run with the build tree checked out at `--start`."""
    start = io.git("rev-parse", "--verify", f"{args.start}^{{commit}}")
    text = Path(args.results).read_text(encoding="utf-8")
    reply = json.loads(text[text.index("{") : text.rindex("}") + 1])
    rows = baselines_from(args.trial, start, reply)
    for row in rows:
        manifest.append_baseline(row)
    failed = sum(r.outcome == "fail" for r in rows)
    print(f"Recorded {len(rows)} checks at {start[:12]}: {failed} failed there.")
    return 0
