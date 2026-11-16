"""Shared fixtures. The repository is public, so tests build small synthetic beads and never
read the machine's real database."""

from __future__ import annotations

import json
from pathlib import Path

import pytest


@pytest.fixture(autouse=True)
def cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Every test's `$BERYL_CACHE`, so none touches the machine's cache."""
    path = tmp_path / "cache"
    monkeypatch.setenv("BERYL_CACHE", str(path))
    return path


def record(bead_id: str, **fields: object) -> str:
    """One line of a synthetic `bd export`."""
    base: dict[str, object] = {
        "_type": "issue",
        "id": bead_id,
        "title": f"Title of {bead_id}",
        "status": "open",
        "priority": 2,
        "issue_type": "task",
        "created_at": "2026-11-01T10:00:00Z",
        "updated_at": "2026-11-02T10:00:00Z",
    }
    base.update(fields)
    return json.dumps(base)


def parent(child: str, of: str) -> dict[str, str]:
    return {"issue_id": child, "depends_on_id": of, "type": "parent-child"}


def blocker(blocked: str, by: str) -> dict[str, str]:
    return {"issue_id": blocked, "depends_on_id": by, "type": "blocks"}


# A small project: an epic with two children, one of them blocked by a bead outside it, a
# flaky check inside a fix-run epic, a bug report, a deferred bead and a closed bead.
EXPORT: list[str] = [
    record("by-epic", title="An epic", issue_type="epic", priority=1),
    record(
        "by-epic.1",
        title="First child",
        status="in_progress",
        assignee="local_ff743724-15be-4f39-9c98-3aba110a68a7",
        dependencies=[parent("by-epic.1", "by-epic")],
        description="Needs **by-free** first. See `by-epic` and <b>tags</b>.",
        comments=[
            {
                "author": "Someone",
                "text": "A *note*.",
                "created_at": "2026-11-03T00:00:00Z",
            }
        ],
    ),
    record(
        "by-epic.2",
        title="Second child",
        status="closed",
        closed_at="2026-11-04T00:00:00Z",
        close_reason="Done in abc123",
        dependencies=[parent("by-epic.2", "by-epic"), blocker("by-epic.2", "by-free")],
    ),
    record("by-free", title="A free bead", priority=0, labels=["tier:opus"]),
    record(
        "by-ci",
        title="Flaky check",
        labels=["flaky-check"],
        dependencies=[parent("by-ci", "by-epic")],
    ),
    record(
        "by-bug", title="Arrow looks wrong", issue_type="bug", labels=["bug-report"]
    ),
    record("by-later", title="Someday", status="deferred"),
    record("by-old", title="Closed long ago", status="closed"),
    "not json",
]
