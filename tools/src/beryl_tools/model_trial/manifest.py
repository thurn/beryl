"""The records of a model trial, one JSON lines file per kind, in
`$BERYL_CACHE/model-trial/<trial epic>/`. They are never committed.

- `pairs.jsonl`: one row per change to a pair. A pair's latest row is its state.
- `grades.jsonl`: one row per arm of each graded pair.
"""

from __future__ import annotations

import dataclasses
import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, TypeVar

from .. import paths

TICKET_ID: re.Pattern[str] = re.compile(r"[A-Z][A-Za-z0-9]{1,15}(\.[0-9]+)*")

# A pair's states, in order: recorded from the spec's list, marked for a block, filed as a
# source update and a copy. `skipped` is a listed ticket that `pair add` refused, or one that
# `pair file` found claimed or closed.
LISTED = "listed"
MARKED = "marked"
FILED = "filed"
SKIPPED = "skipped"


def root() -> Path:
    return paths.cache_root() / "model-trial"


def trial_dir(trial: str) -> Path:
    """The directory of one trial's records, named by its epic's ID."""
    if not TICKET_ID.fullmatch(trial):
        raise ValueError(f"not a ticket ID: {trial!r}")
    path = root() / trial
    path.mkdir(parents=True, exist_ok=True)
    return path


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


T = TypeVar("T")


def _build(cls: type[T], data: dict[str, Any]) -> T:
    """`cls` from a row, ignoring keys it does not know, so that old rows stay readable."""
    names = {f.name for f in dataclasses.fields(cls)}
    return cls(**{k: v for k, v in data.items() if k in names})


def _read(path: Path) -> list[dict[str, Any]]:
    if not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def _append(path: Path, row: object) -> None:
    with path.open("a", encoding="utf-8", newline="\n") as out:
        out.write(json.dumps(dataclasses.asdict(row), sort_keys=True) + "\n")


@dataclass
class Plant:
    """A false premise written into the source's text: `find` was replaced by `replace`.
    `expect` is what a good response to it looks like."""

    find: str
    replace: str
    expect: str


@dataclass
class Pair:
    pair: str  # A random token, used in the copy's `trial:pair-<token>` label.
    trial: str
    source: str
    order: int  # The source's position in the spec's list.
    state: str = LISTED
    kind: str = "other"
    # The source's allocation before `pair add` parked it; restored by `pair file`.
    previous_allocation: int | None = None
    block: int | None = None
    copy: str | None = None
    copy_title: str | None = None
    start_commit: str | None = None
    # The model ID each arm resolved to when the pair was filed, from `ignite.toml`.
    models: dict[str, str] = field(default_factory=dict)
    plant: dict[str, str] | None = None
    # Each change `pair file` made beyond the source and the copy: an epic or ticket it opened,
    # as {"ticket", "field", "before", "after"}.
    opened: list[dict[str, Any]] = field(default_factory=list)
    # The source's attachments copied onto the copy, which grading leaves out.
    copied_attachments: list[str] = field(default_factory=list)
    filed_at: str | None = None
    note: str | None = None
    audit: dict[str, str] | None = None
    # A suspect pair's decision: {"decision": "void" or "clear", "note"}.
    contamination: dict[str, str] | None = None
    updated_at: str = ""


def pairs_path(trial: str) -> Path:
    return trial_dir(trial) / "pairs.jsonl"


def read_pairs(trial: str) -> list[Pair]:
    """Each pair's latest row, in list order."""
    latest: dict[str, Pair] = {}
    for row in _read(pairs_path(trial)):
        pair = _build(Pair, row)
        latest[pair.pair] = pair
    return sorted(latest.values(), key=lambda p: p.order)


def save_pair(pair: Pair) -> None:
    pair.updated_at = now()
    _append(pairs_path(pair.trial), pair)


def find_pair(pairs: list[Pair], key: str) -> Pair:
    """The pair whose token, source or copy is `key`."""
    for pair in pairs:
        if key in (pair.pair, pair.source, pair.copy):
            return pair
    raise KeyError(f"no pair {key!r} in this trial")


@dataclass
class Evidence:
    """One sign that an arm saw the other: `what` matched, `where` it was found."""

    what: str
    where: str


@dataclass
class Grade:
    pair: str
    trial: str
    arm: str  # `source` or `copy`
    ticket: str
    # landed, closed (with a branch), released, stopped or open.
    ended: str
    finished: bool
    start_commit: str | None = None
    end_commit: str | None = None
    # pass, fail or unknown: the stored `ignite check` (copy) or landing gate (source) runs
    # of `end_commit`.
    check: str = "unknown"
    dollars: float | None = None
    tokens: dict[str, int] = field(default_factory=dict)
    agent_minutes: float | None = None
    model_minutes: float | None = None
    claim_to_close_minutes: float | None = None
    files_changed: int = 0
    lines_added: int = 0
    lines_removed: int = 0
    test_lines_added: int = 0
    unnamed_files: list[str] = field(default_factory=list)
    filed_tickets: list[str] = field(default_factory=list)
    comments: list[dict[str, str]] = field(default_factory=list)
    attachments: list[str] = field(default_factory=list)
    models: list[str] = field(default_factory=list)
    # The pair's verdicts, the same on both of its rows.
    void: list[str] = field(default_factory=list)
    suspect: list[dict[str, str]] = field(default_factory=list)
    graded_at: str = ""


def grades_path(trial: str) -> Path:
    return trial_dir(trial) / "grades.jsonl"


def read_grades(trial: str) -> list[Grade]:
    return [_build(Grade, row) for row in _read(grades_path(trial))]


def append_grade(grade: Grade) -> None:
    grade.graded_at = now()
    _append(grades_path(grade.trial), grade)
