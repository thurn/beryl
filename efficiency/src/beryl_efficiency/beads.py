"""The beads source: the stored format used by older imported ticket snapshots
(docs/efficiency-dashboard.md, "Sources").

Later queries use each bead's id, parent, labels, status, its creation, start and close
times, and the Enact metadata (`enact_*`) that ties it to sessions, worktrees and commits.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Callable
from pathlib import Path

from .ingest import Row, SnapshotSource, SourceError


def main_checkout(start: Path) -> Path:
    """The main checkout of the repository at `start`, where the beads database lives; `start`
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
    """The `beads` row and `labels` rows of one bead record. Raises TypeError or KeyError for
    a record that is not a bead."""
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


class Beads(SnapshotSource):
    name = "beads"
    owns = ("beads", "labels", "ticket_aliases")

    def __init__(self, read: Callable[[], bytes]) -> None:
        self._read = read

    def read(self) -> bytes:
        return self._read()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        try:
            records = json.loads(data)
        except ValueError as e:
            raise SourceError(f"the snapshot contains no JSON: {e}") from e
        if not isinstance(records, list):
            raise SourceError("the snapshot contains no list of beads")
        rows: list[Row] = []
        skipped = 0
        for record in records:
            try:
                rows += rows_of(record)
            except (TypeError, KeyError):
                skipped += 1
        return rows, skipped
