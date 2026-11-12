"""The nightly's failures, filed where a session picks them up: when a nightly step that logs
checks (the fuzz, the motion capture, the oracle sweep) fails on origin/staging, its failed
run-log checks go to a bead, so a card a rules change broke reaches someone the next morning
instead of only `$BERYL_CACHE/nightly`.

Each lane's new failures get one P1 `role:steward` bead, labeled `nightly-failure`, naming the
commit, the failed checks, how to replay them and the tail of the night's output. It is a
Steward's, with no parent, because that is the one kind of bead every Enact vizier takes up
whatever its run (Enact's `schedule`); P1 puts it after slow CI's P0 failures. It carries no
`enact_ci_os` or `enact_ci_range`, so Enact never mistakes it for a slow-CI failure, and the
host that failed in `enact_ci_host`, so this machine's viziers take it first.

A check that an open nightly bead already names is not filed again: the next night's repeat
failure comments on that bead instead (the checks are in its metadata, [`CHECKS_KEY`]). A
night with no failed check files nothing. Nothing here fails the nightly: a bd call that fails
is reported and the run goes on (verdicts.Beads).
"""

from __future__ import annotations

import json
import os
import shlex
import socket
from collections.abc import Iterable
from datetime import datetime

from . import runlog, verdicts

LABEL = "nightly-failure"
# The metadata key of the checks a nightly bead was filed for, a line each: a card's name in a
# check has spaces.
CHECKS_KEY = "beryl_nightly_checks"
PRIORITY = "1"
# How much of the night's output a bead quotes.
TAIL_LINES = 40
TAIL_CHARS = 4000


def failed_checks(commit: str, since: datetime) -> dict[str, list[str]]:
    """The nightly's failed checks on `commit` logged from `since` on, by lane, each once."""
    found: dict[str, list[str]] = {}
    for r in runlog.read_log(since):
        if (
            r.get("script") == "nightly"
            and r.get("commit") == commit
            and r.get("outcome") == "fail"
        ):
            checks = found.setdefault(str(r.get("lane")), [])
            if str(r["check"]) not in checks:
                checks.append(str(r["check"]))
    return found


def tail(text: str) -> str:
    """The end of the night's output, short enough for a bead."""
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
    return "just nightly"


def host() -> str:
    """This machine as Enact names it (its `host_name`), so its viziers find the bead theirs."""
    return os.environ.get("ENACT_HOST") or socket.gethostname().split(".")[0].lower()


def open_beads(store: verdicts.Beads) -> dict[str, set[str]]:
    """Each open nightly bead, with the checks it was filed for."""
    _, out = store.bd("list", "--label", LABEL, "-n", "0", "--json")
    beads = verdicts.parse_json(out)
    found: dict[str, set[str]] = {}
    for b in beads if isinstance(beads, list) else []:
        if not isinstance(b, dict) or b.get("status") == "closed":
            continue
        meta = b.get("metadata")
        checks = str(meta.get(CHECKS_KEY, "")) if isinstance(meta, dict) else ""
        found[str(b.get("id"))] = set(checks.splitlines())
    return found


def body(commit: str, lane: str, checks: list[str], passed: str, output: str) -> str:
    since = (
        f" The last nightly that passed every step was at {passed}." if passed else ""
    )
    listed = "\n".join(f"- `{c}`" for c in checks)
    return (
        f"The nightly (`by nightly`, tools/src/beryl_tools/nightly.py) failed {len(checks)} "
        f"{lane} check(s) on origin/staging at {commit}.{since}\n\n{listed}\n\n"
        f"Replay them on that commit with `{reproduce(lane, checks)}`; the run log "
        f"(`by ci-report`) has every record. Slow CI does not run these: `just ci` passing "
        f"proves nothing here.\n\nOutput tail:\n```\n{tail(output)}\n```"
    )


def file(
    commit: str,
    failed: dict[str, list[str]],
    output: str,
    passed: str = "",
    store: verdicts.Beads | None = None,
) -> list[str]:
    """Files the night's failed checks (`failed_checks`): a comment on each open bead that
    already names one of them, and a new bead per lane for the rest. Returns a line per bead
    touched."""
    store = store or verdicts.STORE
    known = open_beads(store) if failed else {}
    lines: list[str] = []
    for lane, checks in sorted(failed.items()):
        for bead, theirs in sorted(known.items()):
            again = [c for c in checks if c in theirs]
            if again:
                code, _ = store.bd(
                    "comments", "add", bead, body(commit, lane, again, passed, output)
                )
                lines.append(
                    f"nightly: {bead}: still failing: {', '.join(again)}"
                    if code == 0
                    else f"nightly: {bead}: comment not added"
                )
        new = [c for c in checks if not any(c in t for t in known.values())]
        if not new:
            continue
        code, out = store.bd(
            "create",
            "--title",
            f"Fix nightly {lane} failure: {new[0]}"
            + (f" and {len(new) - 1} more" if len(new) > 1 else ""),
            "--description",
            body(commit, lane, new, passed, output),
            "--acceptance",
            f"`{reproduce(lane, new)}` passes at the staging tip.",
            "--priority",
            PRIORITY,
            "--labels",
            f"role:steward,{LABEL}",
            "--metadata",
            json.dumps({CHECKS_KEY: "\n".join(new), "enact_ci_host": host()}),
            "--silent",
        )
        lines.append(
            f"nightly: {out.strip()}: filed {', '.join(new)}"
            if code == 0
            else f"nightly: {lane}: bead not filed for {', '.join(new)}"
        )
    return lines
