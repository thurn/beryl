"""Reruns: how a step that failed names its failed checks for its one rerun
(checks.py, `run_lane`), and whether the rerun proved each of them a flake."""

from __future__ import annotations

import json
import re
from collections.abc import Callable

from . import paths

# A step's exit code when it failed to build or start (a compile error, a UiError, an oracle not
# built at its pin): its checks never ran, so a failure on rerun would only cost the time.
CANNOT_RUN = 125

# Given a failed step's output, the checks it names as failed, each an argument the step takes
# to run only that check; none to rerun the step whole.
Failed = Callable[[str], list[str]]


def whole(output: str) -> list[str]:
    """A step that cannot name its failed checks: it reruns whole."""
    return []


def unproven(
    first: list[dict[str, object]], rerun: list[dict[str, object]], step: str
) -> list[str]:
    """The checks that failed in `first` but did not pass in `rerun`, the step's own record
    (`step`, logged when it named no check) aside."""
    passed = {r["check"] for r in rerun if r["outcome"] in ("pass", "pending-pass")}
    failed = (
        str(r["check"])
        for r in first
        if r["outcome"] in ("fail", "pending-fail") and r["check"] != step
    )
    return [c for c in dict.fromkeys(failed) if c not in passed]


# What each rerunnable step names as failed (`Failed`).

# nextest's line for a test that failed, crashed or timed out: its binary and its name.
NEXTEST_FAILED: re.Pattern[str] = re.compile(
    r"^\s*(?:TRY \d+ )?(?:FAIL|LEAK-FAIL|SIG[A-Z]+|ABORT|TIMEOUT) \[[^\]]*\]"
    r"(?: \(\s*\d+/\d+\))? (\S+) (\S+)\s*$",
    re.MULTILINE,
)
ALLOCATIONS_FAILED = "rust-tests: allocations FAILED"
DOCTESTS_FAILED = "rust-tests: doctests FAILED"


def failed_tests(output: str) -> list[str]:
    """The tests nextest failed, each as `<binary> <test>` (`rust_tests_step` takes them); none
    when the allocations harness or a doctest failed, which nextest does not run."""
    if ALLOCATIONS_FAILED in output or DOCTESTS_FAILED in output:
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


def failed_scripts(output: str) -> list[str]:
    """The checks `script-check` names on its `FAILED: script:<id>` lines (script_checks.py);
    none when a session panicked before its first view, which no check's rerun would show again.
    """
    found = re.findall(r"^FAILED: (script:\S+)$", output, re.MULTILINE)
    if "script:unwatched" in found:
        return []
    return list(dict.fromkeys(found))


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


# pytest's summary line for a test that failed or errored: its node id, then ` - ` and why.
PYTEST_FAILED: re.Pattern[str] = re.compile(
    r"^(?:FAILED|ERROR) (tests/\S+?\.py::\S+?)(?: - .*)?$", re.MULTILINE
)


def failed_pytests(output: str) -> list[str]:
    """The node ids of the tests pytest's summary names as failed or errored (python_tests.py
    takes them); none when one errored outside a test, such as on collection."""
    if re.search(r"^ERROR (?!tests/\S+?\.py::)", output, re.MULTILINE):
        return []
    return list(dict.fromkeys(PYTEST_FAILED.findall(output)))
