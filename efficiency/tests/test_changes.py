"""Efficiency changes: validation of `data/efficiency-changes.json`, the commit that added an
entry, and the before and after medians around the moment a commit reached `staging`."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

# `ignite_metrics` registers the Ignite series in `METRICS`, as the command line does, so an
# entry may name `ignite.master-lag-minutes`.
from beryl_efficiency import changes, facts, ignite_metrics, store
from beryl_efficiency.changes import (
    Change,
    compare,
    file_versions,
    problems,
    split_metric,
)
from beryl_efficiency.metrics import METRICS

AT = "2026-11-10T12:00:00Z"
SHA = "a" * 40


def known(_: str) -> bool:
    return True


def entry(**fields: str | None) -> Change:
    given = {"label": "Slow CI keeps its build tree", "metric": "bead.dollars"}
    given.update({k: v for k, v in fields.items() if v is not None})
    return Change(
        str(given["label"]),
        str(given["metric"]),
        str(fields.get("expect") or "down"),
        fields.get("commit"),
    )


def test_the_changes_file_is_valid() -> None:
    """Every entry in the repository's file names a known metric, an expected direction and
    one sentence. Whether an entry's commit is on `staging` depends on which branches this
    checkout has, so the commit lookup is left to the report and is answered `True` here.
    """
    found = changes.load()
    assert found, "the file lists no changes"
    assert problems(found, METRICS, known) == []


def test_an_unknown_metric_fails_validation() -> None:
    found = problems([entry(metric="bead.nonsense[tier=sonnet]")], METRICS, known)
    assert found == [
        "'Slow CI keeps its build tree': no metric bead.nonsense; "
        "`just efficiency metrics` lists them"
    ]


def test_a_missing_commit_fails_validation() -> None:
    assert problems([entry(commit=SHA)], METRICS, lambda _: False) == [
        f"'Slow CI keeps its build tree': commit {SHA} is not on staging"
    ]
    assert problems([entry(commit="abc123")], METRICS, known) == [
        "'Slow CI keeps its build tree': commit must be a full 40-character hash"
    ]


def test_labels_and_directions_are_checked() -> None:
    found = problems(
        [
            entry(label="Two sentences. Here is the second"),
            entry(label="lower case start"),
            entry(label="`ignite test` starts with a code span"),
            entry(expect="sideways", label="Fine label"),
            entry(label="Fine label"),
        ],
        METRICS,
        known,
    )
    assert found == [
        "'Two sentences. Here is the second': the label must be one sentence",
        "'lower case start': the label must be one sentence",
        "'Fine label': expect must be up or down, not 'sideways'",
        "'Fine label': two entries have this label",
    ]


def test_a_metric_filter_chooses_a_cohort() -> None:
    name, scope = split_metric("bead.dollars[tier=sonnet, host=jarvis,epic=by-1]")
    assert name == "bead.dollars"
    assert scope.labels == ("tier:sonnet",)
    assert (scope.host, scope.epic, scope.provider) == ("jarvis", "by-1", None)
    assert scope.range.since is None
    for bad in ("bead.dollars[]", "bead.dollars[tier]", "Ticket dollars"):
        with pytest.raises(ValueError):
            split_metric(bad)


def test_an_entry_is_dated_by_the_commit_that_added_it() -> None:
    def version(*labels: str) -> bytes:
        return json.dumps(
            [{"label": l, "metric": "bead.dollars", "expect": "down"} for l in labels]
        ).encode()

    found = changes.introduced(
        [
            ("first", version("One change")),
            ("broken", b"{not json"),
            ("second", version("One change", "Another change")),
        ]
    )
    assert found == {"One change": "first", "Another change": "second"}


def test_git_returns_each_version_of_the_changes_file_in_order(tmp_path: Path) -> None:
    git = [
        "git",
        "-C",
        str(tmp_path),
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
    ]
    subprocess.run([*git, "init", "-q", "-b", "staging"], check=True)
    path = tmp_path / changes.PATH
    path.parent.mkdir()
    expected: list[tuple[str, bytes]] = []
    for version in (b"[]\n", b'[{"label":"Second version"}]\n'):
        path.write_bytes(version)
        subprocess.run([*git, "add", changes.PATH], check=True)
        subprocess.run(
            [*git, "-c", "commit.gpgsign=false", "commit", "-qm", "change"], check=True
        )
        sha = subprocess.check_output([*git, "rev-parse", "HEAD"], text=True).strip()
        expected.append((sha, version))
    assert file_versions(tmp_path) == expected


def points(day: int, values: list[float]) -> list[tuple[str, float]]:
    """Samples on day `day` of November 2026, an hour apart from 00:00."""
    return [(f"2026-11-{day:02d}T{h:02d}:00:00Z", v) for h, v in enumerate(values)]


def test_medians_are_taken_from_the_week_either_side_of_staging() -> None:
    samples = [
        *points(2, [100.0]),  # more than a week before: left out
        *points(5, [10.0, 12.0, 14.0, 16.0, 18.0]),
        ("2026-11-10T11:59:59Z", 20.0),  # just before the commit reached staging
        ("2026-11-10T12:00:00Z", 1.0),  # the moment it did: after
        *points(12, [2.0, 3.0, 4.0, 5.0]),
        *points(18, [500.0]),  # more than a week after: left out
    ]
    found = compare(samples, AT, "down", 1)
    assert (found.before, found.before_count) == (15.0, 6)
    assert (found.after, found.after_count) == (3.0, 5)
    assert found.verdict == "moved down as expected"
    assert compare(samples, AT, "up", 1).verdict == "moved down, not as expected"


def test_too_few_samples_on_one_side_gives_no_verdict() -> None:
    samples = [*points(5, [1.0, 2.0, 3.0, 4.0, 5.0]), *points(11, [1.0, 1.0, 1.0, 1.0])]
    found = compare(samples, AT, "down", 1)
    assert (found.before_count, found.after_count) == (5, 4)
    assert found.after == 1.0
    assert found.verdict == changes.TOO_FEW


def test_a_bead_is_dated_by_its_start() -> None:
    """A ticket that started before a change and closed after it counts as before."""
    bead = facts.Bead(
        "by-1",
        None,
        None,
        "task",
        "closed",
        "2026-11-09T00:00:00Z",
        "2026-11-10T11:00:00Z",
        "2026-11-11T00:00:00Z",
        frozenset(),
        {},
    )
    assert bead.began() == "2026-11-10T11:00:00Z"


def test_the_query_marks_each_change_from_the_store(cache: Path) -> None:
    """Requests' context in the store, a commit that reached staging, and an entry added by
    that commit give a mark with both medians; an entry not on staging yet has none."""
    db = store.open_store(cache / "store.sqlite")
    rows = [
        (f"m{i}", t, v)
        for i, (t, v) in enumerate(
            [
                *points(8, [100.0, 110.0, 120.0, 130.0, 140.0]),
                *points(11, [60.0, 70.0, 80.0, 90.0, 95.0]),
            ]
        )
    ]
    db.executemany(
        "INSERT INTO requests (host, provider, transcript, message, time, context_tokens)"
        " VALUES ('h', 'claude', 't', ?, ?, ?)",
        rows,
    )
    db.execute(
        "INSERT INTO commits (sha, time, subject) VALUES (?, ?, 'perf: smaller context')",
        (SHA, AT),
    )
    db.commit()
    listed = [
        entry(label="Smaller context", metric="request.context-tokens"),
        entry(label="Not landed", metric="request.context-tokens"),
    ]
    found = changes.marks(db, facts.load(db), listed, {"Smaller context": SHA})
    assert [(m.label, m.time, m.verdict) for m in found] == [
        ("Smaller context", AT, "moved down as expected"),
        ("Not landed", None, "not on staging yet"),
    ]
    assert (found[0].before, found[0].after) == (120.0, 80.0)
    assert (found[0].before_count, found[0].after_count) == (5, 5)
    result = changes.report(
        db, facts.load(db), split_metric("x")[1].range, listed, {"Smaller context": SHA}
    )
    assert "Smaller context" in "\n".join(result.lines)
    db.close()
