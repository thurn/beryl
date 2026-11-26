"""What the trial tool reads from and writes to the world: tickets through `ignite ticket ...
--json`, costs through `just efficiency ... --json`, commits through git in the main checkout,
landing-gate verdicts from the Ignite daemon's database, and model IDs from `ignite.toml`. Everything else in the package is pure, so that its tests
pass made-up ticket JSON and never call any of these.
"""

from __future__ import annotations

import json
import os
import re
import sqlite3
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path
from typing import Any

from .. import paths


class IgniteError(RuntimeError):
    pass


def _run(args: list[str], cwd: Path | None = None) -> str:
    run = subprocess.run(
        args,
        cwd=cwd or paths.REPO,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if run.returncode:
        raise IgniteError(
            f"`{' '.join(args)}` failed ({run.returncode}): {run.stderr.strip() or run.stdout.strip()}"
        )
    return run.stdout


def ignite(*args: str) -> Any:
    out = _run(["ignite", *args, "--json"])
    return json.loads(out) if out.strip() else None


def show(
    ids: list[str], *, history: bool = False, comments: bool = False
) -> list[dict[str, Any]]:
    if not ids:
        return []
    flags = [*(["--history"] if history else []), *(["--comments"] if comments else [])]
    found = ignite("ticket", "show", *ids, *flags)
    if isinstance(found, dict) and "tickets" in found:
        found = found["tickets"]
    return found if isinstance(found, list) else [found]


def ticket(
    ticket_id: str, *, history: bool = False, comments: bool = False
) -> dict[str, Any]:
    return show([ticket_id], history=history, comments=comments)[0]


def with_ancestors(tickets: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """`tickets` and every ancestor of theirs, by ID."""
    known = {str(t["id"]): t for t in tickets}
    missing = {str(t["parent"]) for t in tickets if t.get("parent")} - set(known)
    while missing:
        for found in show(sorted(missing)):
            known[str(found["id"])] = found
        missing = {str(t["parent"]) for t in known.values() if t.get("parent")} - set(
            known
        )
    return known


def open_tickets() -> list[dict[str, Any]]:
    """Every open and pending ticket, with the detail `show` gives (labels, `blocked_by`)."""
    listed: list[dict[str, Any]] = []
    for status in ("open", "pending_approval"):
        found = ignite("ticket", "list", "--status", status, "--limit", "100000")
        listed += found if isinstance(found, list) else found.get("tickets", [])
    ids = [str(t["id"]) for t in listed]
    detail: list[dict[str, Any]] = []
    for start in range(0, len(ids), 200):
        detail += show(ids[start : start + 200])
    return detail


def batch(body: dict[str, Any]) -> dict[str, Any]:
    with tempfile.NamedTemporaryFile(
        "w", suffix=".json", encoding="utf-8", delete=False
    ) as out:
        json.dump(body, out)
        path = Path(out.name)
    try:
        return ignite("ticket", "batch", str(path))
    finally:
        path.unlink(missing_ok=True)


def set_allocation(ticket_id: str, allocation: int | None) -> None:
    """`ignite ticket update` has no way to clear an allocation, so this goes through a batch."""
    batch(
        {"ops": [{"op": "update", "id": ticket_id, "set": {"allocation": allocation}}]}
    )


def comment(ticket_id: str, text: str) -> None:
    _run(["ignite", "ticket", "comment", ticket_id, text])


def attachments(ticket_id: str) -> list[dict[str, Any]]:
    found = ignite("ticket", "attachment", "list", ticket_id)
    return list(found.get("attachments", [])) if isinstance(found, dict) else []


def get_attachment(ticket_id: str, name: str, out: Path) -> Path:
    out.parent.mkdir(parents=True, exist_ok=True)
    _run(
        ["ignite", "ticket", "attachment", "get", ticket_id, name, "--output", str(out)]
    )
    return out


def attach(ticket_id: str, file: Path, name: str, caption: str | None) -> None:
    args = [
        "ignite",
        "ticket",
        "attach",
        ticket_id,
        str(file),
        "--name",
        name,
        "--no-comment",
    ]
    _run([*args, *(["--caption", caption] if caption else [])])


def machine_workers() -> set[str]:
    """The machines whose pool takes tickets: those in the `active` state."""
    fleet = ignite("fleet")
    machines = (
        fleet.get("settings", {}).get("machines", {}) if isinstance(fleet, dict) else {}
    )
    return {name for name, m in machines.items() if m.get("state") == "active"}


def this_machine() -> str:
    """This machine's name in the fleet."""
    status = ignite("status")
    machine = str(status.get("machine", ""))
    for m in ignite("fleet").get("machines", []):
        if m.get("id") == machine:
            return str(m.get("name", machine))
    return machine


def _pool() -> dict[str, Any]:
    with (paths.REPO / "ignite.toml").open("rb") as f:
        return dict(tomllib.load(f).get("pool", {}))


def model_ids(provider: str) -> dict[str, str]:
    """`[pool.models.<provider>]` in `ignite.toml`: the model ID of each tier."""
    models = _pool().get("models", {}).get(provider, {})
    return {str(k): str(v) for k, v in models.items()}


def claude_program() -> str:
    """`[pool.cli.claude] program` in `ignite.toml`, the program the daemon runs Claude with.
    Its `args` are left out: they skip permission prompts, and the reviewer may only read.
    """
    return str(_pool().get("cli", {}).get("claude", {}).get("program", "claude"))


def run_reviewer(argv: list[str], cwd: Path) -> str:
    """Runs one reviewer and returns its stream-json output. Its exit code is not checked:
    the output's `result` event says whether it answered."""
    run = subprocess.run(
        argv,
        cwd=cwd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
        timeout=3 * 60 * 60,
    )
    return run.stdout


def snapshot(commit: str, dest: Path) -> None:
    """The repository's files at `commit`, without its history, in `dest`. A worktree would
    let the reviewer read commit messages and refs, which name tickets and trial branches.
    """
    dest.mkdir(parents=True, exist_ok=True)
    archive = subprocess.Popen(
        ["git", "archive", "--format=tar", commit],
        cwd=main_checkout(),
        stdout=subprocess.PIPE,
    )
    assert archive.stdout is not None
    with tarfile.open(fileobj=archive.stdout, mode="r|") as tar:
        tar.extractall(dest, filter="data")
    if archive.wait():
        raise IgniteError(f"`git archive {commit}` failed ({archive.returncode})")


def diff_text(start: str, end: str) -> str:
    return git("diff", "--no-color", start, end) + "\n"


ISO_TIME: re.Pattern[str] = re.compile(r"[0-9T:.+Z-]+")


def plan_usage(since: str) -> dict[str, list[dict[str, Any]]]:
    """The Claude plan's readings: `before`, each window's last reading before `since` (an ISO
    time), and `now`, each window's latest. A row is `window`, `at` and `percent_used`.
    """
    if not ISO_TIME.fullmatch(since):
        return {"before": [], "now": []}
    # SQLite takes the bare columns of a row from the one that holds `max(at)`.
    query = (
        "SELECT window, max(at) AS at, percent_used FROM plan_usage "
        "WHERE provider = 'claude' {} GROUP BY window ORDER BY window"
    )
    return {
        "before": sql(query.format(f"AND at < '{since}'")),
        "now": sql(query.format("")),
    }


def efficiency(*args: str) -> Any:
    out = _run(["just", "efficiency", *args, "--json"])
    return json.loads(out) if out.strip() else None


def sql(query: str) -> list[dict[str, Any]]:
    """The rows of a read-only query of the efficiency store, as dicts by column."""
    found = efficiency("sql", "--no-ingest", query)
    columns = [str(c) for c in found.get("columns", [])]
    return [dict(zip(columns, row)) for row in found.get("rows", [])]


def gate_passed(tree: str) -> bool | None:
    """Whether the landing gate (`ignite check`) passed on the git tree `tree` on this machine:
    `None` when the Ignite daemon's database cannot be read.

    The daemon keeps one `verdicts` row for each exact state of a change that passed the gate,
    keyed by the git tree ID of the ticket tree's files, and keeps no row for a run that failed.
    It is in `$IGNITE_HOME/daemon.db`, by default `~/.config/ignite/daemon.db`, and it deletes
    rows older than 30 days."""
    try:
        rows = _daemon_rows("SELECT 1 FROM verdicts WHERE tree = ? LIMIT 1", (tree,))
    except sqlite3.Error:
        return None
    return None if rows is None else bool(rows)


def _daemon_rows(query: str, params: tuple[str, ...]) -> list[tuple[Any, ...]] | None:
    """The rows of a read-only query of the Ignite daemon's database, or `None` when it is
    missing."""
    home = os.environ.get("IGNITE_HOME") or str(Path.home() / ".config" / "ignite")
    path = Path(home) / "daemon.db"
    if not path.is_file():
        return None
    with sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=30) as db:
        return db.execute(query, params).fetchall()


def check_failures(session: str, since: str) -> dict[str, str] | None:
    """The checks that failed in the last `ignite check` that `session` started at or after
    `since` (an ISO time), each with its failure message, or `None` when the session ran no
    check then or the daemon's database cannot be read.

    The daemon keeps each command's reply in `handles`. A check that failed before on the
    same inputs is reported only as "failed before on these inputs", so its message is taken
    from the session's latest earlier reply that gave one. A build failure is listed under
    `build:<product>`."""
    try:
        rows = _daemon_rows(
            "SELECT started_at, reply FROM handles WHERE verb = 'check' AND session = ? "
            "ORDER BY started_at",
            (session,),
        )
    except sqlite3.Error:
        return None
    said: dict[str, str] = {}
    last: dict[str, str] | None = None
    for started_at, reply in rows or []:
        try:
            found = (json.loads(reply or "{}").get("json")) or {}
        except json.JSONDecodeError:
            continue
        if "failures" not in found:
            continue  # a `--plan` reply, which runs nothing
        failed: dict[str, str] = {}
        for f in found.get("failures") or []:
            name, message = str(f.get("check")), str(f.get("message") or "")
            if message != FAILED_BEFORE:
                said[name] = message
            failed[name] = said.get(name, message)
        for product in found.get("build_failures") or []:
            failed[f"build:{product}"] = ""
        if str(started_at) >= since:
            last = failed
    return last


FAILED_BEFORE = "failed before on these inputs"


def main_checkout() -> Path:
    return paths.main_checkout() or paths.REPO


def git(*args: str) -> str:
    return _run(["git", *args], cwd=main_checkout()).strip()


def git_ok(*args: str) -> bool:
    try:
        git(*args)
    except IgniteError:
        return False
    return True


STAGING = "origin/staging"


def staging_tip() -> str:
    git("fetch", "--quiet", "origin", "staging")
    return git("rev-parse", STAGING)


def export() -> list[dict[str, Any]]:
    """Every ticket with its comments and history (`ignite ticket export`): about 100 MB, read
    once per grading run to find the tickets and comments each arm's session left elsewhere.
    """
    out = _run(["ignite", "ticket", "export"])
    return [json.loads(line) for line in out.splitlines() if line.strip()]
