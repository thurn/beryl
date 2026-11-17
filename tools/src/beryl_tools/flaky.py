"""A flaky check files a bead to delete it.

A check is flaky when it failed and then passed on its rerun on the same tree (`run_lane` in
checks.py). The flake fails no lane, but tests here must fail deterministically, so each flaky
check gets a P1 bead asking for it to be deleted, or replaced by a fast deterministic test of the
same behaviour. There is no quarantine: a check either can fail CI or is gone.

A check's first flake files nothing. Its bead is filed when it flakes again in another run (on
another commit or machine) within [`REPEAT_DAYS`]; the verdicts recorded on its `ci-check` bead
(verdicts.py) hold the earlier flakes. Until this rule, 56 of the 69 checks that flaked between
2026-11-11 and 2026-11-16 flaked only once, and most of the beads they filed were closed as
problems of the run rather than the check: a sweep at utility priority, Vite reloading pages,
concurrent runs sharing a fixture directory, an exporter changing between attempts. Each of
those beads cost an Opus session. A check that flakes less than once a week costs one rerun a
week, which is cheaper.

Each bead and each later comment quotes the end of the failed attempt's output ([`tail`]),
and the whole of it is kept in `$BERYL_CACHE/ci/flakes` ([`keep`]) for [`KEEP_DAYS`]. Before
this, the run printed only the rerun's passing output, and the Beryl 3D sweep overwrites its
output file on every firing, so the six flaky-check beads open on 2026-11-17 for steps that
name no checks of their own each asked an Opus session to find a failure nothing had kept.

Each check has at most one open bead, labeled [`LABEL`] with the check in its metadata
([`CHECK_KEY`]). While it is open, a later flake of the same check adds a comment to it instead
of filing another. Nothing here fails a run: a bd call that fails is reported and the run goes
on (verdicts.Beads).
"""

from __future__ import annotations

import re
import socket
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import paths, verdicts

LABEL = "flaky-check"
CHECK_KEY = "beryl_flaky_check"
PRIORITY = "1"
# How recent an earlier flake of the same check must be for a flake to file a bead.
REPEAT_DAYS = 7
# How much of the failed attempt's output a bead or comment quotes.
TAIL_LINES = 40
TAIL_CHARS = 3000
# How long a failed attempt's whole output is kept.
KEEP_DAYS = 14


@dataclass(frozen=True)
class Flake:
    """A flaky check's lane, and whether the check is a whole step (`lane:step`, logged when the
    step names no checks of its own, runlog.Run.log) rather than one test."""

    lane: str
    whole: bool = False


def open_beads(store: verdicts.Beads) -> dict[str, str]:
    """The open flaky-check beads, by the check each was filed for."""
    beads = store.list_issues("--label", LABEL)
    found: dict[str, str] = {}
    for b in beads if isinstance(beads, list) else []:
        if not isinstance(b, dict) or b.get("status") == "closed":
            continue
        meta = b.get("metadata")
        if isinstance(meta, dict) and isinstance(meta.get(CHECK_KEY), str):
            found[meta[CHECK_KEY]] = str(b.get("id"))
    return found


def seen(script: str, commit: str, host: str | None = None) -> str:
    host = (host or socket.gethostname()).split(".")[0]
    return f"`by {script}` on {host} at {commit[:10] or 'an unknown commit'}"


def earlier(
    history: list[verdicts.Entry], script: str, commit: str, host: str
) -> list[verdicts.Entry]:
    """The check's flakes in the last `REPEAT_DAYS` from other runs than this one. A run is told
    apart by its script, commit and machine, so the flake this run just recorded, and one in a
    concurrent run of the same tree, do not count."""
    since = verdicts.now() - timedelta(days=REPEAT_DAYS)
    found: list[verdicts.Entry] = []
    for e in history:
        if e.get("verdict") != "flaky":
            continue
        try:
            when = datetime.fromisoformat(str(e.get("time")))
        except ValueError:
            continue
        if when < since:
            continue
        if (e.get("script"), e.get("commit"), e.get("host")) == (script, commit, host):
            continue
        found.append(e)
    return found


def tail(output: str) -> str:
    """The end of a failed attempt's output, short enough for a bead."""
    return "\n".join(output.rstrip().splitlines()[-TAIL_LINES:])[-TAIL_CHARS:]


def folder() -> Path:
    return paths.cache_root() / "ci" / "flakes"


def keep(check: str, output: str, where: Path | None = None) -> Path | None:
    """Writes a failed attempt's whole `output` to a file of its own under `where` (`folder`
    by default) and returns it, after removing files there older than [`KEEP_DAYS`]; None when
    there is no output or it cannot be written."""
    if not output.strip():
        return None
    where = where or folder()
    try:
        where.mkdir(parents=True, exist_ok=True)
        old = time.time() - KEEP_DAYS * 86400
        for f in where.glob("*.txt"):
            if f.stat().st_mtime < old:
                f.unlink(missing_ok=True)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
        name = re.sub(r"[^A-Za-z0-9._-]+", "_", check)[:120]
        # A run's flakes in the same second, or a concurrent run's, each get a file.
        for n in range(1000):
            path = where / f"{stamp}-{name}{f'-{n}' if n else ''}.txt"
            try:
                with open(path, "x") as handle:
                    handle.write(output)
                return path
            except FileExistsError:
                continue
        return None
    except OSError:
        return None


def quoted(output: str, kept: Path | None, host: str) -> str:
    """The failed attempt's output as a bead quotes it: its tail and where the rest is."""
    if not output.strip():
        return "The failed attempt printed nothing."
    where = f" The whole output is in `{kept}` on {host}." if kept else ""
    return f"The end of the failed attempt's output:{where}\n\n```\n{tail(output)}\n```"


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


def body(
    check: str,
    flake: Flake,
    where: str,
    before: list[verdicts.Entry],
    failure: str = "",
) -> str:
    what = (
        f"The step `{check}`, which names no checks of its own,"
        if flake.whole
        else f"The check `{check}` (lane `{flake.lane}`)"
    )
    find = (
        "First find which test or check inside the step failed, from the output above. Then"
        " treat that one as the flaky check.\n\n"
        if flake.whole
        else ""
    )
    return (
        f"{what} failed and then passed on its rerun on the same tree in {where}. It had"
        f" flaked the same way {len(before)} time(s) in the {REPEAT_DAYS} days before, in other"
        f" runs: {', '.join(was(e) for e in before)}. The flake failed no lane, but checks must fail deterministically, so the flaky one is deleted"
        " instead of being rerun until it passes. `just ci-report` lists its runs, failures"
        f" and verdicts; its bead of verdicts is {verdicts.bead_id(check)}.\n\n"
        f"{failure + chr(10) * 2 if failure else ''}{find}"
        "Before deleting it, ask: could a fast, deterministic test (a Rust unit test, a Node"
        " test with no browser, a check against a recorded fixture) keep most of what this"
        " check protects? If so, write that test, then delete this check. If not, delete it"
        " when it has never caught a real bug (no `catch` verdict), or move it to the nightly"
        " when it has. If the flake comes from a race in the product rather than in the test,"
        " fix the race instead and say so.\n\n"
        "Record on this bead which you chose and why." + verdicts.SKILL_NOTE
    )


def was(e: verdicts.Entry) -> str:
    commit = str(e.get("commit") or "")
    return seen(str(e.get("script")), commit, str(e.get("host") or "")) + (
        f" ({e.get('time')})"
    )


def report(
    flakes: dict[str, Flake],
    script: str,
    commit: str,
    store: verdicts.Beads | None = None,
    host: str | None = None,
    failures: dict[str, str] | None = None,
) -> list[str]:
    """Comments on the open bead of each check in `flakes` that has one, and files a bead for
    each other check that flaked in another run in the last `REPEAT_DAYS` (`earlier`); returns
    a line for each. Each comment and bead quotes the check's failed attempt from `failures`
    ([`quoted`]), whose whole output is kept ([`keep`])."""
    if not flakes:
        return []
    store = store or verdicts.STORE
    host = host or socket.gethostname()
    known = open_beads(store)
    where = seen(script, commit, host)
    # One `bd export` for all of them, and none when every flaky check has its bead open.
    history = store.history([c for c in flakes if c not in known])
    lines: list[str] = []
    short = host.split(".")[0]
    for check, flake in sorted(flakes.items()):
        output = (failures or {}).get(check, "")
        kept = keep(check, output)
        if kept:
            lines.append(f"flaky: {check}: its failed attempt's output is in {kept}")
        failure = quoted(output, kept, short)
        bead = known.get(check)
        if bead:
            store.add_comment(bead, f"Flaked again in {where}.\n\n{failure}")
            lines.append(f"flaky: {check}: {bead} is open to delete it")
            continue
        before = earlier(history.get(check, []), script, commit, host)
        if not before:
            lines.append(
                f"flaky: {check}: its first flake in {REPEAT_DAYS} days;"
                " a second in another run files a bead to delete it"
            )
            continue
        code, out = store.create_issue(
            title=title(check, flake),
            body=body(check, flake, where, before, failure),
            acceptance=acceptance(check, flake),
            priority=PRIORITY,
            labels=f"{LABEL},tier:opus",
            metadata={CHECK_KEY: check},
        )
        if code != 0:
            lines.append(f"flaky: {check}: delete bead not filed")
            continue
        known[check] = out.strip()
        lines.append(f"flaky: {check}: filed {out.strip()} to delete it")
    return lines
