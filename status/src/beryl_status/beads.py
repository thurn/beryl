"""The beads as the site shows them: read from `bd export` (one JSON object per line, each with
its labels, dependencies and comments), and indexed by parent, children and blockers."""

from __future__ import annotations

import json
import subprocess
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from pathlib import Path

# The label the bug button (⌘B in the app) puts on the beads it files.
BUG_REPORT = "bug-report"
# The groups of beads that CI, Enact and the other tools file by themselves, by the labels
# they put on them, in the order the home page shows them. A bead with any of a group's
# labels is in that group; the first group that matches wins.
AUTOMATIC: tuple[tuple[str, frozenset[str]], ...] = (
    ("Slow CI failures", frozenset({"role:steward"})),
    ("Incidents", frozenset({"role:justiciar"})),
    ("Flaky checks", frozenset({"flaky-check"})),
    (
        "Sweep and nightly failures",
        frozenset({"sweep-failure", "nightly-failure", "b3d-sweep"}),
    ),
    ("CI time budget", frozenset({"ci-cut", "check-budget"})),
    ("CI structure review", frozenset({"ci-structure", "ci-structure-log"})),
    ("Count gate", frozenset({"count-creep", "count-lower"})),
    ("Check verdicts", frozenset({"ci-check"})),
)
DEFERRED = "deferred"
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


def automatic_group(bead: Bead) -> str | None:
    """The name of the automatic group the bead's labels put it in, if any."""
    labels = set(bead.labels)
    return next((name for name, marks in AUTOMATIC if labels & marks), None)


@dataclass
class Home:
    """The home page's categories. Each open bead is in at most one, checked in this order:
    deferred beads, bug reports and the automatic groups wherever they sit (a fix run often
    gathers them under an epic), then epics and the remaining beads outside any epic. A bead
    inside an epic shows on its epic's page instead."""

    epics: list[Bead] = field(default_factory=list)
    beads: list[Bead] = field(default_factory=list)
    automatic: list[tuple[str, list[Bead]]] = field(default_factory=list)
    bug_reports: list[Bead] = field(default_factory=list)
    deferred: list[Bead] = field(default_factory=list)


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

    def home(self) -> Home:
        """The open beads the home page shows, by category (`Home`)."""
        found = Home()
        groups: dict[str, list[Bead]] = {}
        # Sorting only the open beads (a few hundred of thousands) keeps every rebuild cheap.
        for bead in sorted(
            (b for b in self.beads.values() if not b.closed), key=by_attention
        ):
            top = bead.id not in self.parent
            group = automatic_group(bead)
            if bead.status == DEFERRED:
                found.deferred.append(bead)
            elif BUG_REPORT in bead.labels:
                found.bug_reports.append(bead)
            elif group is not None:
                groups.setdefault(group, []).append(bead)
            elif top and bead.issue_type == "epic":
                found.epics.append(bead)
            elif top:
                found.beads.append(bead)
        found.automatic = [
            (name, groups[name]) for name, _ in AUTOMATIC if name in groups
        ]
        return found

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
