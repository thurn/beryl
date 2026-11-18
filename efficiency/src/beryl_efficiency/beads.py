"""Ticket rows and the main checkout (docs/efficiency-dashboard.md, "Sources").

`rows_of` builds the `beads` and `labels` rows of one ticket record; `ignite.HubTickets` is
the only caller. Later queries use each ticket's id, parent, labels, status, its creation,
start and close times, and the metadata keys starting `enact_` that tie it to sessions,
worktrees and commits (tickets from before Ignite carry them as imported; `ignite.py` sets
the ones it can for newer tickets). `main_checkout` and `run_command` find the checkout and
run `git` for `commits.py`.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from .ingest import Row, SourceError


def main_checkout(start: Path) -> Path:
    """The main checkout of the repository at `start`, whose `.git` directory the worktrees share; `start`
    itself when git cannot say."""
    try:
        found = subprocess.run(
            ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
            cwd=start,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return start
    common = Path(found)
    return common.parent if common.name == ".git" else common


def run_command(cwd: Path, command: list[str]) -> bytes:
    """Read stdout, reporting command failures through ingest's source tally."""
    try:
        done = subprocess.run(command, cwd=cwd, capture_output=True, check=False)
    except OSError as e:
        raise SourceError(f"{' '.join(command)}: {e}") from e
    if done.returncode != 0:
        message = done.stderr.decode(errors="replace").strip().splitlines()
        raise SourceError(
            f"{' '.join(command)} exited {done.returncode}: {message[-1] if message else ''}"
        )
    return done.stdout


def text(record: dict[str, object], key: str) -> str | None:
    value = record.get(key)
    if value is None:
        return None
    if not isinstance(value, str):
        raise TypeError(f"{key} is not text")
    return value


def rows_of(record: object) -> list[Row]:
    """The `beads` row and `labels` rows of one ticket record. Raises TypeError or KeyError for
    a record that is not a ticket."""
    if not isinstance(record, dict):
        raise TypeError("not an object")
    bead = record["id"]
    if not isinstance(bead, str) or not bead:
        raise TypeError("id is not text")
    metadata = record.get("metadata") or {}
    if not isinstance(metadata, dict):
        raise TypeError("metadata is not an object")
    enact = {k: v for k, v in metadata.items() if str(k).startswith("enact_")}
    priority = record.get("priority")
    labels = record.get("labels") or []
    if not isinstance(labels, list):
        raise TypeError("labels is not a list")
    rows: list[Row] = [
        (
            "beads",
            {
                "id": bead,
                "title": text(record, "title"),
                "parent": text(record, "parent"),
                "issue_type": text(record, "issue_type"),
                "status": text(record, "status"),
                "priority": priority if isinstance(priority, int) else None,
                "created_at": text(record, "created_at"),
                "started_at": text(record, "started_at"),
                "closed_at": text(record, "closed_at"),
                "enact": json.dumps(enact, sort_keys=True),
            },
        )
    ]
    rows += [("labels", {"bead": bead, "label": str(label)}) for label in labels]
    return rows
