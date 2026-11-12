"""A check run over its budget files a bead to audit its slowest steps; it fails nothing.

The budgets (`checks.FAST_BUDGET`, `checks.SLOW_BUDGET`) say how long `by check` and `by ci`
should take. A run over one passes when its checks pass, and the overrun goes to a bead, so the
steps get sped up, moved to slow CI or deleted without blocking the push that noticed. A slower
machine than the one the budgets were set on, or one shared with other work, still pushes.

Each script has one open bead at a time, labeled [`LABEL`] with the script in its metadata
([`SCRIPT_KEY`]); while it is open, a later overrun prints its id and files nothing. Nothing
here fails a run: a bd call that fails is reported and the run goes on (verdicts.Beads).
"""

from __future__ import annotations

import json
import socket
import subprocess

from . import paths, verdicts

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
