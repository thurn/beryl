"""The beads as the site shows them: read from `bd export` (one JSON object per line, each with
its labels, dependencies and comments), and indexed by parent, children and blockers."""

from __future__ import annotations

import json
import subprocess
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from pathlib import Path

# The labels of the beads that CI and the bug button file by themselves, which the home page
# leaves out. The same list as the `beads` recipe in the justfile, which prints the same beads
# as text.
SYSTEM_LABELS: frozenset[str] = frozenset(
    {
        "bug-report",
        "flaky-check",
        "ci-cut",
        "check-budget",
        "sweep-failure",
        "nightly-failure",
        "role:steward",
    }
)
CLOSED = "closed"
# The order statuses are shown in, most active first; any other status sorts after them.
STATUS_ORDER: tuple[str, ...] = ("in_progress", "blocked", "open", "deferred", CLOSED)


@dataclass(frozen=True)
class Comment:
    author: str
    text: str
    created_at: str


@dataclass(frozen=True)
class Bead:
    id: str
    title: str
    status: str
    priority: int
    issue_type: str
    created_at: str
    updated_at: str
    assignee: str = ""
    owner: str = ""
    created_by: str = ""
    started_at: str = ""
    closed_at: str = ""
    close_reason: str = ""
    description: str = ""
    acceptance_criteria: str = ""
    design: str = ""
    notes: str = ""
    labels: tuple[str, ...] = ()
    # (kind, id) for each bead this one depends on: `parent-child` names its parent, `blocks`
    # a bead that blocks it, and other kinds (`related`, `discovered-from`) a looser link.
    depends_on: tuple[tuple[str, str], ...] = ()
    comments: tuple[Comment, ...] = ()
    metadata: tuple[tuple[str, str], ...] = ()

    @property
    def closed(self) -> bool:
        return self.status == CLOSED


def text(value: object) -> str:
    return "" if value is None else str(value)


def parse_line(record: Mapping[str, object]) -> Bead | None:
    """One exported record as a bead, or None for a record that is not an issue."""
    if record.get("_type", "issue") != "issue" or not record.get("id"):
        return None
    raw_deps = record.get("dependencies")
    deps: list[tuple[str, str]] = []
    if isinstance(raw_deps, list):
        for d in raw_deps:
            if isinstance(d, dict) and d.get("depends_on_id"):
                deps.append((text(d.get("type")), text(d.get("depends_on_id"))))
    raw_comments = record.get("comments")
    comments: list[Comment] = []
    if isinstance(raw_comments, list):
        for c in raw_comments:
            if isinstance(c, dict):
                comments.append(
                    Comment(
                        text(c.get("author")),
                        text(c.get("text")),
                        text(c.get("created_at")),
                    )
                )
    raw_labels = record.get("labels")
    labels = (
        tuple(sorted(text(x) for x in raw_labels))
        if isinstance(raw_labels, list)
        else ()
    )
    raw_metadata = record.get("metadata")
    metadata = (
        tuple(sorted((text(k), text(v)) for k, v in raw_metadata.items()))
        if isinstance(raw_metadata, dict)
        else ()
    )
    priority = record.get("priority")
    return Bead(
        id=text(record.get("id")),
        title=text(record.get("title")),
        status=text(record.get("status")) or "open",
        priority=priority if isinstance(priority, int) else 2,
        issue_type=text(record.get("issue_type")) or "task",
        created_at=text(record.get("created_at")),
        updated_at=text(record.get("updated_at")),
        assignee=text(record.get("assignee")),
        owner=text(record.get("owner")),
        created_by=text(record.get("created_by")),
        started_at=text(record.get("started_at")),
        closed_at=text(record.get("closed_at")),
        close_reason=text(record.get("close_reason")),
        description=text(record.get("description")),
        acceptance_criteria=text(record.get("acceptance_criteria")),
        design=text(record.get("design")),
        notes=text(record.get("notes")),
        labels=labels,
        depends_on=tuple(deps),
        comments=tuple(sorted(comments, key=lambda c: c.created_at)),
        metadata=metadata,
    )


def parse(lines: Iterable[str]) -> list[Bead]:
    """The beads in an export; a line that is not JSON is skipped."""
    found: list[Bead] = []
    for line in lines:
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(record, dict) and (bead := parse_line(record)) is not None:
            found.append(bead)
    return found


def export(main: Path) -> str:
    """Every bead, as `bd export` prints it to standard output, run in the main checkout."""
    return subprocess.run(
        ["bd", "export"],
        cwd=main,
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def status_rank(status: str) -> int:
    return STATUS_ORDER.index(status) if status in STATUS_ORDER else len(STATUS_ORDER)


def by_attention(bead: Bead) -> tuple[int, int, str]:
    """Most active status first, then highest priority (P0), then most recently updated."""
    return (status_rank(bead.status), bead.priority, invert(bead.updated_at))


def invert(timestamp: str) -> str:
    """A string that sorts in the reverse order of an ISO timestamp."""
    return "".join(chr(0x10FFFF - ord(c)) for c in timestamp)


@dataclass
class Index:
    """The beads by id, with each one's parent, children, blockers and the beads it blocks."""

    beads: dict[str, Bead]
    parent: dict[str, str] = field(default_factory=dict)
    children: dict[str, list[str]] = field(default_factory=dict)
    blocked_by: dict[str, list[str]] = field(default_factory=dict)
    blocking: dict[str, list[str]] = field(default_factory=dict)
    related: dict[str, list[str]] = field(default_factory=dict)

    @staticmethod
    def build(beads: Iterable[Bead]) -> Index:
        index = Index({b.id: b for b in beads})
        for bead in index.beads.values():
            for kind, other in bead.depends_on:
                if other not in index.beads:
                    continue
                if kind == "parent-child":
                    index.parent[bead.id] = other
                    index.children.setdefault(other, []).append(bead.id)
                elif kind == "blocks":
                    index.blocked_by.setdefault(bead.id, []).append(other)
                    index.blocking.setdefault(other, []).append(bead.id)
                else:
                    index.related.setdefault(bead.id, []).append(other)
                    index.related.setdefault(other, []).append(bead.id)
        return index

    def get(self, ids: Iterable[str]) -> list[Bead]:
        return sorted((self.beads[i] for i in set(ids)), key=by_attention)

    def epic(self, bead_id: str) -> Bead | None:
        """The bead's parent, the epic it belongs to, if it has one."""
        parent = self.parent.get(bead_id)
        return self.beads.get(parent) if parent else None

    def top_level(self) -> list[Bead]:
        """The beads `just beads` lists: open beads with no parent, without the beads CI and
        the bug button file."""
        return sorted(
            (
                b
                for b in self.beads.values()
                if not b.closed
                and b.id not in self.parent
                and not SYSTEM_LABELS.intersection(b.labels)
            ),
            key=by_attention,
        )

    def descendants(self, bead_id: str) -> list[Bead]:
        """Every bead under this one, children's children included."""
        found: list[Bead] = []
        stack = list(self.children.get(bead_id, []))
        seen: set[str] = set()
        while stack:
            current = stack.pop()
            if current in seen:
                continue
            seen.add(current)
            found.append(self.beads[current])
            stack.extend(self.children.get(current, []))
        return found
