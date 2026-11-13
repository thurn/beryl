"""A flaky check files a bead to delete it.

A check is flaky when it failed and then passed on its rerun on the same tree (`run_lane` in
checks.py). The flake fails no lane, but tests here must fail deterministically, so each flaky
check gets a P1 bead asking for it to be deleted, or replaced by a fast deterministic test of the
same behaviour. There is no quarantine: a check either can fail CI or is gone.

Each check has at most one open bead, labeled [`LABEL`] with the check in its metadata
([`CHECK_KEY`]). While it is open, a later flake of the same check adds a comment to it instead
of filing another. Nothing here fails a run: a bd call that fails is reported and the run goes
on (verdicts.Beads).
"""

from __future__ import annotations

import json
import socket
from dataclasses import dataclass

from . import verdicts

LABEL = "flaky-check"
CHECK_KEY = "beryl_flaky_check"
PRIORITY = "1"


@dataclass(frozen=True)
class Flake:
    """A flaky check's lane, and whether the check is a whole step (`lane:step`, logged when the
    step names no checks of its own, runlog.Run.log) rather than one test."""

    lane: str
    whole: bool = False


def open_beads(store: verdicts.Beads) -> dict[str, str]:
    """The open flaky-check beads, by the check each was filed for."""
    _, out = store.bd("list", "--label", LABEL, "-n", "0", "--json")
    beads = verdicts.parse_json(out)
    found: dict[str, str] = {}
    for b in beads if isinstance(beads, list) else []:
        if not isinstance(b, dict) or b.get("status") == "closed":
            continue
        meta = b.get("metadata")
        if isinstance(meta, dict) and isinstance(meta.get(CHECK_KEY), str):
            found[meta[CHECK_KEY]] = str(b.get("id"))
    return found


def seen(script: str, commit: str) -> str:
    host = socket.gethostname().split(".")[0]
    return f"`by {script}` on {host} at {commit[:10] or 'an unknown commit'}"


def title(check: str, flake: Flake) -> str:
    if flake.whole:
        return f"Delete the flaky test in step {check}"
    return f"Delete flaky check {check}"


def acceptance(check: str, flake: Flake) -> str:
    """What closes the bead. A step's own record stands for every test in it, so the step stays
    and only the flaky test inside it goes."""
    done = "or it is replaced by a deterministic test, and this bead records which and why."
    if flake.whole:
        return f"The flaky test inside `{check}` no longer runs in CI, {done} The step stays."
    return f"`{check}` no longer runs in CI, {done}"


def body(check: str, flake: Flake, where: str) -> str:
    what = (
        f"The step `{check}`, which names no checks of its own,"
        if flake.whole
        else f"The check `{check}` (lane `{flake.lane}`)"
    )
    find = (
        "First find which test or check inside the step failed: the slow-CI and fast-check"
        " logs of the failing run show its output. Then treat that one as the flaky check.\n\n"
        if flake.whole
        else ""
    )
    return (
        f"{what} failed and then passed on its rerun on the same tree in {where}. The flake"
        " failed no lane, but checks must fail deterministically, so the flaky one is deleted"
        " instead of being rerun until it passes. `just ci-report` lists its runs, failures"
        f" and verdicts; its bead of verdicts is {verdicts.bead_id(check)}.\n\n{find}"
        "Before deleting it, ask: could a fast, deterministic test (a Rust unit test, a Node"
        " test with no browser, a check against a recorded fixture) keep most of what this"
        " check protects? If so, write that test, then delete this check. If not, delete it"
        " when it has never caught a real bug (no `catch` verdict), or move it to the nightly"
        " when it has. If the flake comes from a race in the product rather than in the test,"
        " fix the race instead and say so.\n\n"
        "Record on this bead which you chose and why. The `flakefix` skill"
        " (`.agents/skills/flakefix/SKILL.md`) says how to find the cause and close it."
    )


def report(
    flakes: dict[str, Flake],
    script: str,
    commit: str,
    store: verdicts.Beads | None = None,
) -> list[str]:
    """Files a bead for each check in `flakes` that has none open, and comments on the open bead
    of each that has one; returns a line for each."""
    if not flakes:
        return []
    store = store or verdicts.STORE
    known = open_beads(store)
    where = seen(script, commit)
    lines: list[str] = []
    for check, flake in sorted(flakes.items()):
        bead = known.get(check)
        if bead:
            store.bd("comments", "add", bead, f"Flaked again in {where}.")
            lines.append(f"flaky: {check}: {bead} is open to delete it")
            continue
        code, out = store.bd(
            "create",
            "--title",
            title(check, flake),
            "--description",
            body(check, flake, where),
            "--acceptance",
            acceptance(check, flake),
            "--priority",
            PRIORITY,
            "--labels",
            f"{LABEL},tier:opus",
            "--metadata",
            json.dumps({CHECK_KEY: check}),
            "--silent",
        )
        if code != 0:
            lines.append(f"flaky: {check}: delete bead not filed")
            continue
        known[check] = out.strip()
        lines.append(f"flaky: {check}: filed {out.strip()} to delete it")
    return lines
