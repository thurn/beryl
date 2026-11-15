"""`just efficiency changes`: whether each change made to save time or tokens moved the metric
it was meant to move (docs/efficiency-dashboard.md, "Efficiency changes").

The changes are listed in `data/efficiency-changes.json`. An entry's commit is the one that
added it to the file, found from the file's history on `staging`, because a commit cannot hold
its own hash and `enact push` rebases. An entry made before the file existed names its commit
in `commit` instead. The moment a commit reached `staging` is its committer time, which
`enact push` sets when it rebases onto `staging` just before pushing.

For each entry, the metric's samples in the seven days before that moment and in the seven
days after it give two medians. Samples are dated as `metrics.py` dates them, by when their
unit began, so a bead that started before the change and closed after it counts as before.
"""

from __future__ import annotations

import json
import re
import sqlite3
import subprocess
from collections.abc import Callable, Collection, Iterable
from dataclasses import asdict, dataclass
from datetime import timedelta
from pathlib import Path

# Imported for the metrics they register, so that every metric name is known here.
from . import cards, ci, machine  # noqa: F401
from .commits import History
from .facts import Facts, spread
from .metrics import METRICS, Mark, Points, fmt
from .render import Result, describe, table
from .rollups import day_of
from .scope import Range, Scope, parse_time

PATH = "data/efficiency-changes.json"
# How far before and after a change its samples are taken from.
WEEK = timedelta(days=7)
# The fewest samples on each side that give a verdict.
MINIMUM = 5
EXPECT = ("up", "down")
KEYS: frozenset[str] = frozenset({"label", "metric", "expect", "commit"})
SHA: re.Pattern[str] = re.compile(r"^[0-9a-f]{40}$")
# A metric with an optional cohort filter: `bead.dollars[tier=sonnet,role=executor]`.
SPEC: re.Pattern[str] = re.compile(r"^([a-z0-9.-]+)(?:\[([^\]]*)\])?$")
# Where one sentence ends and another begins.
SENTENCE_BREAK: re.Pattern[str] = re.compile(r"[.!?;]\s|\n")
TOO_FEW = "too few samples"


@dataclass(frozen=True)
class Change:
    label: str
    metric: str
    expect: str
    commit: str | None


def table_path() -> Path:
    """`data/efficiency-changes.json` in the checkout this package is in."""
    return Path(__file__).resolve().parents[3] / PATH


def parse(data: bytes) -> list[Change]:
    """The entries in `data`. Raises ValueError when it is not a list of objects with the
    fields of an entry; what the fields say is checked by `problems`."""
    entries = json.loads(data)
    if not isinstance(entries, list):
        raise ValueError(f"{PATH} is not a list")
    found: list[Change] = []
    for i, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise ValueError(f"entry {i} is not an object")
        unknown = set(entry) - KEYS
        if unknown:
            raise ValueError(f"entry {i} has unknown fields {sorted(unknown)}")
        values = {k: entry.get(k) for k in KEYS}
        for key, value in values.items():
            if value is not None and not isinstance(value, str):
                raise ValueError(f"entry {i}'s {key} is not text")
        label, metric, expect = (
            values["label"],
            values["metric"],
            values["expect"],
        )
        if not label or not metric or not expect:
            raise ValueError(f"entry {i} needs a label, a metric and an expect")
        commit = values["commit"]
        found.append(
            Change(
                str(label), str(metric), str(expect), str(commit) if commit else None
            )
        )
    return found


def load(path: Path | None = None) -> list[Change]:
    return parse((path or table_path()).read_bytes())


def split_metric(spec: str) -> tuple[str, Scope]:
    """A metric's name and the cohort its filter chooses, over all time. A filter is
    `key=value` pairs separated by commas: `epic`, `host` and `provider` set those filters
    and any other key chooses beads with the label `key:value`, as `--where` does. Raises
    ValueError for a malformed spec."""
    found = SPEC.match(spec)
    if not found:
        raise ValueError(f"{spec!r} is not a metric name with an optional [key=value]")
    name, filters = found.group(1), found.group(2)
    labels: list[str] = []
    named: dict[str, str] = {}
    for part in filters.split(",") if filters is not None else []:
        key, equals, value = (s.strip() for s in part.partition("="))
        if not equals or not key or not value:
            raise ValueError(f"{spec!r}: {part!r} is not key=value")
        if key in ("epic", "host", "provider"):
            named[key] = value
        else:
            labels.append(f"{key}:{value}")
    return name, Scope(
        Range(None, None, "all"),
        tuple(labels),
        named.get("epic"),
        named.get("host"),
        named.get("provider"),
    )


def base(spec: str) -> str:
    """The metric's name without its filter."""
    return spec.partition("[")[0]


def problems(
    changes: Iterable[Change],
    known: Collection[str],
    on_staging: Callable[[str], bool],
) -> list[str]:
    """What is wrong with the entries: a metric that is not in `known` or is malformed, an
    `expect` other than up or down, a `commit` that is not a full hash of a commit on
    `staging`, a label that is not one sentence, or two entries with one label."""
    found: list[str] = []
    seen: set[str] = set()
    for c in changes:
        where = f"{c.label!r}:"
        if c.label in seen:
            found.append(f"{where} two entries have this label")
        seen.add(c.label)
        if not one_sentence(c.label):
            found.append(f"{where} the label must be one sentence")
        try:
            name, _ = split_metric(c.metric)
        except ValueError as e:
            found.append(f"{where} {e}")
        else:
            if name not in known:
                found.append(
                    f"{where} no metric {name}; `just efficiency metrics` lists them"
                )
        if c.expect not in EXPECT:
            found.append(f"{where} expect must be up or down, not {c.expect!r}")
        if c.commit is not None:
            if not SHA.match(c.commit):
                found.append(f"{where} commit must be a full 40-character hash")
            elif not on_staging(c.commit):
                found.append(f"{where} commit {c.commit} is not on staging")
    return found


def one_sentence(label: str) -> bool:
    """Whether `label` is one sentence: it starts with a capital letter, and no sentence ends
    inside it."""
    text = label.strip()
    return (
        bool(text)
        and text == label
        and text[0].isupper()
        and not SENTENCE_BREAK.search(text)
    )


def introduced(versions: Iterable[tuple[str, bytes]]) -> dict[str, str]:
    """For each label in the file's history, the first commit whose version holds it.
    `versions` is each (commit, contents), oldest first; a version that does not parse is
    skipped."""
    found: dict[str, str] = {}
    for sha, data in versions:
        try:
            entries = parse(data)
        except ValueError:
            continue
        for c in entries:
            found.setdefault(c.label, sha)
    return found


def git(checkout: Path, *command: str) -> subprocess.CompletedProcess[bytes] | None:
    try:
        return subprocess.run(
            ["git", *command], cwd=checkout, capture_output=True, check=False
        )
    except OSError:
        return None


def file_versions(checkout: Path, ref: str = "staging") -> list[tuple[str, bytes]]:
    """Each commit on `ref` that changed the changes file, oldest first, with the file's
    contents at that commit. Empty where git cannot say."""
    done = git(checkout, "log", "--reverse", "--format=%H", ref, "--", PATH)
    if done is None or done.returncode != 0:
        return []
    found: list[tuple[str, bytes]] = []
    for sha in done.stdout.decode().split():
        shown = git(checkout, "show", f"{sha}:{PATH}")
        if shown is not None and shown.returncode == 0:
            found.append((sha, shown.stdout))
    return found


def on_staging(checkout: Path, ref: str = "staging") -> Callable[[str], bool]:
    """Whether a commit is `ref` or one of its ancestors, asked of git in `checkout`."""

    def check(sha: str) -> bool:
        done = git(checkout, "merge-base", "--is-ancestor", sha, ref)
        return done is not None and done.returncode == 0

    return check


@dataclass(frozen=True)
class Compared:
    before: float | None
    after: float | None
    before_count: int
    after_count: int
    verdict: str


def compare(points: Points, at: str, expect: str, digits: int) -> Compared:
    """The median of the points in the week before `at` and in the week from it, and
    whether the metric moved in the `expect` direction."""
    moment = parse_time(at)
    before = [v for t, v in points if moment - WEEK <= parse_time(t) < moment]
    after = [v for t, v in points if moment <= parse_time(t) < moment + WEEK]
    first = number(spread(before, digits)["median"])
    second = number(spread(after, digits)["median"])
    if len(before) < MINIMUM or len(after) < MINIMUM or first is None or second is None:
        verdict = TOO_FEW
    elif first == second:
        verdict = "did not move"
    elif (second < first) == (expect == "down"):
        verdict = f"moved {expect} as expected"
    else:
        verdict = f"moved {'up' if second > first else 'down'}, not as expected"
    return Compared(first, second, len(before), len(after), verdict)


def number(value: object) -> float | None:
    assert value is None or isinstance(value, (int, float))
    return None if value is None else float(value)


def staging_time(history: History, sha: str | None) -> str | None:
    """When a commit reached `staging`: its committer time, if the store has it."""
    found = history.commits.get(sha) if sha else None
    return found.time if found else None


def mark(
    db: sqlite3.Connection, facts: Facts, change: Change, commit: str | None
) -> Mark:
    """One change's mark: its commit, when that reached staging, and the comparison. An entry
    with no commit on staging yet, or a metric that is not known, has no comparison."""
    time = staging_time(facts.history(), commit)
    found = Compared(None, None, 0, 0, "not on staging yet")
    try:
        name, scope = split_metric(change.metric)
    except ValueError as e:
        found = Compared(None, None, 0, 0, str(e))
    else:
        metric = METRICS.get(name)
        if metric is None:
            found = Compared(None, None, 0, 0, f"no metric {name}")
        elif time is not None:
            points = metric.points(db, facts, scope)
            found = compare(points, time, change.expect, metric.digits)
    return Mark(
        change.label,
        change.metric,
        commit,
        time,
        change.expect,
        found.before,
        found.after,
        found.before_count,
        found.after_count,
        found.verdict,
    )


def marks(
    db: sqlite3.Connection,
    facts: Facts,
    changes: Iterable[Change],
    added: dict[str, str],
    metric: str | None = None,
) -> list[Mark]:
    """The marks of `changes`, each entry's commit being its own `commit` or else the one
    that added it (`added`, from `introduced`); with `metric`, only the changes made to that
    metric, whatever their filters. Ordered by when they reached staging."""
    found = [
        mark(db, facts, c, c.commit or added.get(c.label))
        for c in changes
        if metric is None or base(c.metric) == metric
    ]
    return sorted(found, key=lambda m: (m.time or "~", m.label))


@dataclass
class Changes:
    since: str | None
    range: str
    changes: list[Mark]


def report(
    db: sqlite3.Connection,
    facts: Facts,
    range_: Range,
    changes: Iterable[Change],
    added: dict[str, str],
) -> Result:
    """Every change whose commit reached staging in the range, and every change not on
    staging yet."""
    found = [
        m
        for m in marks(db, facts, changes, added)
        if m.time is None or range_.holds(m.time)
    ]
    result = Changes(range_.since, range_.name, found)
    rows = [
        [
            day_of(m.time) if m.time else "-",
            m.label,
            m.metric,
            m.expect,
            f"{show(m.before, m.metric)} ({m.before_count})",
            f"{show(m.after, m.metric)} ({m.after_count})",
            m.verdict,
        ]
        for m in found
    ]
    return Result(
        "changes",
        asdict(result),
        [
            f"efficiency changes since {result.since or 'the start'} ({result.range}): "
            f"medians over the 7 days before and after each reached staging, "
            f"with sample counts",
            "",
            *table(
                ["staging", "change", "metric", "expect", "before", "after", "verdict"],
                rows,
            ),
        ],
        sorted({f"efficiency metrics {base(m.metric)}" for m in found})[:3],
        describe("since", "changes"),
    )


def show(value: float | None, spec: str) -> str:
    metric = METRICS.get(base(spec))
    return fmt(value, metric.digits if metric else 2)
