"""The nightly's failures, filed where a session picks them up: when a nightly step that logs
checks (the fuzz, the motion capture, the oracle sweep) fails on origin/staging, its failed
run-log checks go to a ticket, so a card a rules change broke reaches someone the next morning
instead of only `$BERYL_CACHE/nightly`.

Each new failure signature gets a P1 ticket, labeled `nightly-failure`, naming the
commit, the failed checks, how to replay them and the tail of the night's output. It is a
Steward's by default, with no parent, because that is the one kind of ticket every Enact vizier takes up
whatever its run (Enact's `schedule`); P1 puts it after slow CI's P0 failures. It carries no
`enact_ci_os` or `enact_ci_range`, so Enact never mistakes it for a slow-CI failure, and the
host that failed in `enact_ci_host`, so this machine's viziers take it first.

A repeat diagnostic comments on its canonical open incident (`incidents.report`) instead
of filing again. Different faults of one check remain separate. The reported checks are
in each ticket's metadata ([`CHECKS_KEY`]). A
night with no failed check files nothing. Nothing here fails the nightly: a ticket request that fails
is reported and the run goes on (verdicts.Tickets).
"""

from __future__ import annotations

import os
import shlex
import socket
from collections.abc import Iterable, Mapping
from datetime import datetime

from . import incidents, runlog, step_results, verdicts

LABEL = "nightly-failure"
# The metadata key of the checks a nightly ticket was filed for, a line each: a card's name in a
# check has spaces.
CHECKS_KEY = "beryl_nightly_checks"
PRIORITY = "1"
# How much of the night's output a ticket quotes.
TAIL_LINES = 40
TAIL_CHARS = 4000


def failed_checks(
    commit: str, since: datetime, script: str = "nightly"
) -> dict[str, list[str]]:
    """The failed checks on `commit` of the runs of `script` (the nightly's by default) logged
    from `since` on, by lane, each once.

    A check whose last record passed is left out: a step that fails reruns its failed checks
    once (`checks.run_lane`), and a pass on that rerun proves the first failure a flake,
    which flaky.py reports on its own. Counting the first failure filed a sweep ticket for 92
    checks when the rerun had passed 67 of them, each a page load that timed out on a loaded
    machine.
    """
    last: dict[str, tuple[str, str]] = {}
    for r in runlog.read_log(since):
        if r.get("script") == script and r.get("commit") == commit:
            check = str(r["check"])
            last[check] = (
                str(r.get("lane")),
                "incomplete" if step_results.incomplete(r) else str(r.get("outcome")),
            )
    found: dict[str, list[str]] = {}
    for check, (lane, outcome) in last.items():
        if outcome == "fail":
            found.setdefault(lane, []).append(check)
    return found


def tail(text: str) -> str:
    """The end of the night's output, short enough for a ticket."""
    return "\n".join(text.splitlines()[-TAIL_LINES:])[-TAIL_CHARS:]


def reproduce(lane: str, checks: Iterable[str]) -> str:
    """The command that replays a lane's failed checks."""
    if lane == "oracle":
        # `oracle:<card>/<scenario>/<engine>`: the cards, each once.
        cards = dict.fromkeys(c.split(":", 1)[1].split("/", 1)[0] for c in checks)
        return "just oracle " + " ".join(shlex.quote(c) for c in cards)
    if lane == "motion":
        return "just motion --more"
    if lane == "fuzz":
        return "just fuzz"
    if lane == "iroh":
        return "just test-rust -p beryl_link_iroh --locked"
    return "just nightly"


def host() -> str:
    """This machine as Enact names it (its `host_name`), so its viziers find the ticket theirs."""
    return os.environ.get("ENACT_HOST") or socket.gethostname().split(".")[0].lower()


def body(
    commit: str, lane: str, checks: list[str], passed: str, output: str, kept: str = ""
) -> str:
    """The ticket's text. `output` is the lane's own step output when the caller has it; `kept`
    names the file that holds the whole of it."""
    since = (
        f" The last nightly that passed every step was at {passed}." if passed else ""
    )
    listed = "\n".join(f"- `{c}`" for c in checks)
    whole = f" The whole output of the {lane} step is in `{kept}`." if kept else ""
    return (
        f"The nightly (`by nightly`, tools/src/beryl_tools/nightly.py) failed {len(checks)} "
        f"{lane} check(s) on origin/staging at {commit}.{since}\n\n{listed}\n\n"
        f"Replay them on that commit with `{reproduce(lane, checks)}`; the run log "
        f"(`by ci-report`) has every record.{whole} Slow CI does not run these: `just ci` "
        f"passing proves nothing here.{verdicts.SKILL_NOTE}\n\n"
        f"Output tail:\n```\n{tail(output)}\n```"
    )


def file(
    commit: str,
    failed: dict[str, list[str]],
    output: str,
    passed: str = "",
    store: verdicts.Tickets | None = None,
    by_lane: Mapping[str, str] | None = None,
    kept: Mapping[str, str] | None = None,
    run_id: str = "",
) -> list[str]:
    """File each failed check's diagnostic on its canonical incident and return a line for each.

    A lane's text quotes its own step's output from `by_lane` (a lane is named after
    its step) and names the file `kept` holds it in; without them, the night's `output`.
    """
    store = store or verdicts.STORE
    lines: list[str] = []
    for lane, checks in sorted(failed.items()):
        own = (by_lane or {}).get(lane, output)
        where = (kept or {}).get(lane, "")
        for check in checks:
            own_check = incidents.check_output(check, own)
            result = store.incident(
                incidents.failure(check, own_check, host()),
                run_id or f"nightly:{commit}",
                host(),
                title=f"Fix nightly {lane} failure: {check}",
                body=body(commit, lane, [check], passed, own_check, where),
                acceptance=f"`{reproduce(lane, [check])}` passes at the staging tip.",
                priority=PRIORITY,
                labels=f"role:steward,{LABEL}",
                metadata={CHECKS_KEY: check, "enact_ci_host": host()},
            )
            lines.append(
                f"nightly: {result.issue}: {'filed' if result.created else 'still failing'}: {check}"
                if result
                else f"nightly: {lane}: ticket not filed for {check}"
            )
    return lines
