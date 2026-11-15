"""Small synthetic sources for the tests."""

from __future__ import annotations

import json
from collections.abc import Mapping
from pathlib import Path

from beryl_efficiency.ingest import LineSource, Row


def bead(id: str, **fields: object) -> dict[str, object]:
    """A bead record as `bd list --json` prints one, with `fields` over the defaults."""
    return {
        "id": id,
        "title": f"Bead {id}",
        "status": "open",
        "priority": 2,
        "issue_type": "task",
        "created_at": "2026-11-01T10:00:00Z",
        **fields,
    }


def listing(*beads: Mapping[str, object]) -> bytes:
    return json.dumps(list(beads)).encode()


class Lines(LineSource):
    """A line source over `folder`'s `*.jsonl` files: each line is an object with `id` and
    `n`, written to the `beads` table as a bead `id` with priority `n`."""

    name = "lines"

    def __init__(self, folder: Path) -> None:
        self.folder = folder

    def files(self) -> list[Path]:
        return sorted(self.folder.glob("*.jsonl"))

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        return [("beads", {"id": str(record["id"]), "priority": int(record["n"])})]
