"""Over-budget checks file beads; they fail nothing.

The fast check (`by check`) has a budget in load-adjusted seconds (`checks.FAST_BUDGET`). A run
over it passes when its checks pass, and files one "Audit slow steps" bead (`report`), labeled
[`LABEL`] with the script in its metadata ([`SCRIPT_KEY`]); while it is open, a later overrun
prints its id and files nothing.

Slow CI (`by ci`) has a budget in CPU seconds ([`SLOW_CPU_BUDGET`]), since the machine is never
quiet and wall seconds measure its load as much as the checks. After each run, `slow_report`
compares a full run's CPU seconds (check_value.full_run: each check's median CPU seconds, summed
over the checks that ran in one of their lane's last [`RUNS`] runs, so one noisy run files
nothing) with the budget. Over it, it files one P1 bead per lane, labeled [`CUT_LABEL`], naming
the checks to cut, chosen lowest value first by check_value.cuts, with each check's runs,
catches and CPU seconds: delete a check that caught nothing in the window, move one that caught
something to the nightly. The checks in [`KEEP`] are never named. While any cut bead is open it files nothing more, and once the last one
closes it counts only the runs that started after it closed, and waits for [`RUNS`] of them, so
the checks that bead cut are not counted, nor named, again.

Nothing here fails a run: a bd call that fails is reported and the run goes on (verdicts.Beads).
"""

from __future__ import annotations

import json
import platform
import socket
import subprocess
from datetime import datetime, timedelta, timezone

from . import check_value, paths, runlog, system, verdicts

LABEL = "check-budget"
SCRIPT_KEY = "beryl_budget_script"
PRIORITY = "2"


def open_bead(script: str, store: verdicts.Beads) -> str | None:
    """The open budget bead filed for `script`, if any."""
    _, out = store.bd("list", "--label", LABEL, "-n", "0", "--json")
    beads = verdicts.parse_json(out)
    for b in beads if isinstance(beads, list) else []:
        if not isinstance(b, dict) or b.get("status") == "closed":
            continue
        meta = b.get("metadata")
        if isinstance(meta, dict) and meta.get(SCRIPT_KEY) == script:
            return str(b.get("id"))
    return None


def commit() -> str:
    run = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        check=False,
    )
    return run.stdout.strip() or "an unknown commit"


def body(script: str, budget: int, took: str, lanes: list[str], load: str) -> str:
    listed = "\n".join(f"- {line}" for line in lanes)
    host = socket.gethostname().split(".")[0]
    return (
        f"`by {script}` took {took} on {host} at {commit()}, over its budget of "
        f"{budget} s (load-adjusted seconds; tools/src/beryl_tools/checks.py). Its checks "
        f"passed: the overrun fails nothing (budget.py).\n\nLanes, slowest steps named:\n"
        f"{listed}\n\n{load}\n\nFind the steps that cost the most (`just ci-report` has "
        f"each check's timings) and speed them up, move them to slow CI "
        f"(`verdicts.SLOW_ONLY`) or delete them, until `by {script}` fits its budget on "
        f"this machine when it is otherwise idle. Do not raise the budget."
    )


def report(
    script: str,
    budget: int,
    took: str,
    lanes: list[str],
    load: str,
    store: verdicts.Beads | None = None,
) -> str:
    """Files a bead for a run of `script` over `budget`, unless one is open; returns a line
    saying which."""
    store = store or verdicts.STORE
    known = open_bead(script, store)
    if known:
        return f"{script}: over budget: {known} is open to audit it"
    code, out = store.bd(
        "create",
        "--title",
        f"Audit slow steps: by {script} over its {budget} s budget",
        "--description",
        body(script, budget, took, lanes, load),
        "--acceptance",
        f"`by {script}` on every lane fits {budget} s on an idle machine.",
        "--priority",
        PRIORITY,
        "--labels",
        f"{LABEL},tier:opus",
        "--metadata",
        json.dumps({SCRIPT_KEY: script}),
        "--silent",
    )
    if code != 0:
        return f"{script}: over budget: bead not filed"
    return f"{script}: over budget: filed {out.strip()} to audit it"


# Slow CI's budget: CPU seconds of a full run (check_value.full_run). Set on 2026-11-13, when a
# full run used about 1,840 CPU seconds, and about 520 once every check that caught no bug in
# 30 days was cut. At 800 the cuts take the costliest of those checks (end-to-end tests, verb
# films, fixtures) and stop before the Rust unit tests that cost a fraction of a second each
# and that the fast check runs too. Never raise it.
SLOW_CPU_BUDGET = 800
# How many runs a full run's checks are taken from (check_value.full_run), and how many runs
# after a cut bead closes are measured before another is filed.
RUNS: int = check_value.RUNS
CUT_LABEL = "ci-cut"
LANE_KEY = "beryl_cut_lane"
CUT_PRIORITY = "1"
# Checks no cut bead names, each guarding something no other check does; `<lane>:*` keeps a
# whole lane.
KEEP: dict[str, str] = {
    "app:smoke": "the desktop app builds and starts at all",
    "mobile:mobile-check": "the phone apps build and start at all",
    "rust:clippy": "the whole workspace builds and passes clippy",
    "ui:tsc": "the UI type-checks in strict mode",
    "names:*": "the repository rules AGENTS.md names, a few CPU seconds in all",
}


def kept(check: str) -> bool:
    return check in KEEP or f"{check.split(':')[0]}:*" in KEEP


def cut_beads(store: verdicts.Beads) -> tuple[list[str], str]:
    """The open cut beads, and the latest time a closed one closed (`""` when none has)."""
    _, out = store.bd("list", "--label", CUT_LABEL, "--all", "-n", "0", "--json")
    beads = verdicts.parse_json(out)
    opened: list[str] = []
    closed = ""
    for b in beads if isinstance(beads, list) else []:
        if not isinstance(b, dict):
            continue
        if b.get("status") != "closed":
            opened.append(str(b.get("id")))
        elif (when := run_time(b.get("closed_at"))) > closed:
            closed = when
    return sorted(opened), closed


def run_time(value: object) -> str:
    """A bd time (`2026-11-13T17:57:58Z`) as the run log writes one, so the two compare as
    strings; `""` when it is not a time."""
    try:
        when = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    except ValueError:
        return ""
    return when.astimezone(timezone.utc).isoformat(timespec="seconds")


def verdict(g: check_value.Group) -> str:
    return "move to the nightly" if g.catches else "delete"


def check_line(g: check_value.Group) -> str:
    """One group's line in a cut bead."""
    names = ", ".join(f"`{c}`" for c in g.checks)
    together = " (failing together, one group)" if len(g.checks) > 1 else ""
    estimated = " (estimated from wall seconds)" if g.estimated else ""
    return (
        f"- {verdict(g)}: {names}{together}: {g.runs} runs, {g.catches} catches,"
        f" {g.cpu:.2f} CPU s a run{estimated}"
    )


def cut_body(
    lane: str, groups: list[check_value.Group], whole: float, budget: float, host: str
) -> str:
    listed = "\n".join(check_line(g) for g in groups)
    saved = sum(g.cpu for g in groups)
    keep = "\n".join(f"- `{c}`: {why}" for c, why in KEEP.items())
    return (
        f"Slow CI on {host} is over its CPU budget: a full run (each check's median CPU"
        f" seconds, summed over the checks of each lane's last {RUNS} runs) uses {whole:.0f}"
        " CPU seconds, and the budget is"
        f" {budget:.0f} (`SLOW_CPU_BUDGET`, tools/src/beryl_tools/budget.py). `just ci-value`"
        " scores every check by the real bugs it caught in the last"
        f" {check_value.DAYS} days per CPU-second, and these are the lowest-value checks of the"
        f" `{lane}` lane among those whose removal brings a full run under the budget, about"
        f" {saved:.0f} CPU seconds a run:\n\n{listed}\n\n"
        "For each check, first ask: could a fast, deterministic test (a Rust unit test, a Node"
        " test with no browser, a check against a recorded fixture) keep most of what this"
        " end-to-end check protects? If so, write that test, then delete the check. If not,"
        " delete it when it caught nothing (`delete` above), or move it to the nightly"
        " (a step in `nightly.STEPS`, tools/src/beryl_tools/nightly.py) when it caught"
        " something (`move to the nightly` above). Either way the check leaves slow CI: when"
        " the fast check (`by check`) runs it too, keep it there and take it out of slow CI"
        " alone. Checks listed together failed in the same"
        " runs and go together. Make the cuts without asking anyone, and record on this bead"
        " what you did with each check and why.\n\nNo cut bead names these checks, which guard"
        f" something no other check does:\n{keep}"
    )


def file_cut(
    lane: str,
    groups: list[check_value.Group],
    whole: float,
    budget: float,
    store: verdicts.Beads,
) -> str:
    host = socket.gethostname().split(".")[0]
    count = sum(len(g.checks) for g in groups)
    code, out = store.bd(
        "create",
        "--title",
        f"Cut {count} checks from slow CI's {lane} lane",
        "--description",
        cut_body(lane, groups, whole, budget, host),
        "--acceptance",
        "Every check listed no longer runs in slow CI: deleted, replaced by a fast"
        " deterministic test, or moved to the nightly; this bead records which for each and"
        " why.",
        "--priority",
        CUT_PRIORITY,
        "--labels",
        f"{CUT_LABEL},tier:opus",
        "--metadata",
        json.dumps({LANE_KEY: lane}),
        "--silent",
    )
    if code != 0:
        return f"ci budget: {lane}: cut bead not filed"
    return f"ci budget: {lane}: filed {out.strip()} to cut {count} checks"


def slow_report(
    records: list[dict[str, object]] | None = None,
    history: dict[str, list[verdicts.Entry]] | None = None,
    store: verdicts.Beads | None = None,
    budget: float = SLOW_CPU_BUDGET,
    host: str | None = None,
) -> list[str]:
    """Compares this host's slow CI with its CPU budget after a run, and files a cut bead per
    lane when it is over and no cut bead is open; returns lines saying what it found."""
    if system.WINDOWS:
        return [
            "ci budget: CPU time is not measured on Windows; the Mac checks the budget"
        ]
    store = store or verdicts.STORE
    opened, closed = cut_beads(store)
    if opened:
        return [f"ci budget: {' '.join(opened)} open to cut checks; nothing more filed"]
    since = datetime.now(timezone.utc) - timedelta(days=check_value.DAYS)
    host = host or platform.node()
    if records is None:
        records = runlog.read_log(since)
    mine = [r for r in records if r.get("host") == host]
    if history is None:
        history = store.history(store.checks())
    scores = check_value.score(mine, history, since).since(closed)
    if len(scores.runs) < RUNS:
        after = f" since a cut bead closed at {closed}" if closed else ""
        return [
            f"ci budget: {len(scores.runs)} of {RUNS} runs{after}; not measured yet"
        ]
    whole = check_value.full_run(scores, set(), RUNS)
    if whole <= budget:
        return [f"ci budget: a full run uses {whole:.0f} of {budget:.0f} CPU seconds"]
    scores.groups = [g for g in scores.groups if not any(kept(c) for c in g.checks)]
    by_lane: dict[str, list[check_value.Group]] = {}
    for g in check_value.cuts(scores, budget, RUNS):
        by_lane.setdefault(scores.lanes[g.checks[0]], []).append(g)
    lines = [f"ci budget: a full run uses {whole:.0f} CPU seconds, over {budget:.0f}"]
    lines += [
        file_cut(lane, groups, whole, budget, store)
        for lane, groups in sorted(by_lane.items())
    ]
    return lines
