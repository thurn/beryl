"""The records of a model trial, one JSON lines file per kind, in
`$BERYL_CACHE/model-trial/<trial epic>/`. They are never committed.

- `pairs.jsonl`: one row per change to a pair. A pair's latest row is its state.
- `grades.jsonl`: one row per arm of each graded pair.
- `reviews.jsonl`: one row per blind reviewer run.
- `verdicts.jsonl`: one row per verdict the user gave on the judging page. His latest verdict on
  a pair counts.
- `judging.json`: which pairs the judging page shows, and which arm is A on each.
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
    # The spec's copy arm this pair's copy runs on. A pair of the first arm is the main pair:
    # it is listed, marked and filed with its source. A pair of any other arm is filed after
    # its main pair, from the main copy's text and the same start commit, and names it in
    # `main`. Rows written before several arms existed belong to the arm named `copy`.
    arm: str = "copy"
    main: str | None = None
    # Titles for the other arms' copies, by arm, set with `pair title --arm` before they are
    # filed. Each must differ from the source's title and from every other copy's.
    arm_titles: dict[str, str] = field(default_factory=dict)
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
    """The pair whose token or copy is `key`, or the main pair of the source `key`."""
    for pair in pairs:
        if key in (pair.pair, pair.copy) or (key == pair.source and pair.main is None):
            return pair
    raise KeyError(f"no pair {key!r} in this trial")


def extra_copies(pairs: list[Pair], main: Pair) -> dict[str, Pair]:
    """The pairs of `main`'s other copy arms that have been filed, by arm."""
    return {p.arm: p for p in pairs if p.main == main.pair}


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
    # pass, fail or unknown: a landed source passed the landing gate; a copy passed when the
    # Ignite daemon stored a passing gate verdict for the tree of `end_commit`.
    check: str = "unknown"
    # True when the efficiency store has no request of the arm's claim or misses part of its
    # time, so `dollars` and `tokens` are too low. The report leaves it out of the dollar
    # medians.
    cost_partial: bool = False
    # True when part of the arm's sessions ran where their transcripts are not on the grading
    # machine, so `agent_minutes` and `model_minutes` are too low. The report leaves it out of
    # the minute medians.
    minutes_partial: bool = False
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


@dataclass
class Review:
    """One run of the blind reviewer on a pair. `answer` is the reviewer's JSON, in its own A
    and B terms; `a_arm` says which arm (`source` or `copy`) it saw as A."""

    pair: str
    trial: str
    run: int
    # The run's A is drawn from `seed`; each later run of the pair swaps A and B.
    seed: int
    a_arm: str
    reviewer_model: str
    # A hash of the reviewer's prompt file, so that a changed prompt is a different rubric.
    rubric_version: str
    ok: bool
    answer: dict[str, Any] = field(default_factory=dict)
    cost_usd: float = 0.0
    attempts: int = 0
    error: str | None = None
    reviewed_at: str = ""


def reviews_path(trial: str) -> Path:
    return trial_dir(trial) / "reviews.jsonl"


def read_reviews(trial: str) -> list[Review]:
    return [_build(Review, row) for row in _read(reviews_path(trial))]


def append_review(review: Review) -> None:
    review.reviewed_at = now()
    _append(reviews_path(review.trial), review)


@dataclass
class Verdict:
    """The user's blind picks on one pair: each `A`, `B` or `none` (no preference), where
    `a_arm` is the arm the page showed as A."""

    pair: str
    trial: str
    a_arm: str
    quality: str
    initiative: str
    overall: str
    judged_at: str = ""


def verdicts_path(trial: str) -> Path:
    return trial_dir(trial) / "verdicts.jsonl"


def read_verdicts(trial: str) -> list[Verdict]:
    """The user's latest verdict on each pair."""
    latest: dict[str, Verdict] = {}
    for row in _read(verdicts_path(trial)):
        verdict = _build(Verdict, row)
        latest[verdict.pair] = verdict
    return list(latest.values())


def append_verdict(verdict: Verdict) -> None:
    verdict.judged_at = now()
    _append(verdicts_path(verdict.trial), verdict)


def latest_grades(grades: list[Grade]) -> dict[str, dict[str, Grade]]:
    """Each graded pair's latest row per arm, by pair token and then arm."""
    found: dict[str, dict[str, Grade]] = {}
    for grade in grades:
        found.setdefault(grade.pair, {})[grade.arm] = grade
    return {p: arms for p, arms in found.items() if {"source", "copy"} <= set(arms)}
