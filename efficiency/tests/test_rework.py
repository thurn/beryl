"""Commits, rework, lead time and the card cohort: the rework rules on small histories, and
the commits source through the command over the query tests' synthetic store."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from beryl_efficiency import cli
from tests.legacy_sources import Beads
from beryl_efficiency.commits import Commit, Commits, History, is_card
from beryl_efficiency.rework import CARD_FIX, SLOW_CI, Charge, compute
from tests.synthetic import git_snapshot
from tests.test_queries import BEADS, NUMBER, numbers_in, root, run

__all__ = ["root"]  # The query tests' store, used as a fixture here.

ONE = "rust/cards/cards_2025/abc/one.rs"
TWO = "rust/cards/cards_2025/abc/two.rs"


def commit(
    sha: str, parent: str | None, minute: int, subject: str, *paths: str
) -> Commit:
    """A commit that changes `paths`; a card file among them is added when its name ends in
    the commit's sha, as in `one.rs` added by `one`."""
    added = frozenset(p for p in paths if is_card(p) and Path(p).stem == sha)
    return Commit(
        sha,
        f"2026-11-15T10:{minute:02d}:00Z",
        subject,
        (parent,) if parent else (),
        frozenset(paths),
        added,
    )


def chain(*commits: Commit) -> list[Commit]:
    return list(commits)


def steward_history(fix_path: str) -> History:
    """Tickets A, B and C land a commit each after a base commit; the Steward S then lands a
    fix that changes `fix_path`."""
    commits = chain(
        commit("base", None, 0, "feat: base", "README.md"),
        commit("a1", "base", 1, "feat: a", "src/a.py"),
        commit("b1", "a1", 2, "feat: b", "src/b.py"),
        commit("c1", "b1", 3, "feat: c", "src/c.py"),
        commit("s1", "c1", 4, "fix: slow ci", fix_path),
    )
    return History(commits, {"A": ["a1"], "B": ["b1"], "C": ["c1"], "S": ["s1"]}, [])


def test_a_steward_charges_the_bead_whose_files_its_fix_touched() -> None:
    found = compute(steward_history("src/b.py"), {"S": "base..c1"}, {"S": 9.0})
    assert found.charges == [Charge("B", "S", SLOW_CI, 9.0)]
    assert found.dollars("A") == 0.0 and found.dollars("C") == 0.0


def test_a_steward_with_no_distinguishing_file_is_shared_by_all() -> None:
    found = compute(steward_history("tests/other.py"), {"S": "base..c1"}, {"S": 9.0})
    assert [(c.bead, c.dollars) for c in found.charges] == [
        ("A", 3.0),
        ("B", 3.0),
        ("C", 3.0),
    ]


def test_a_steward_is_never_its_own_candidate() -> None:
    history = steward_history("src/s.py")
    found = compute(history, {"S": "base..s1"}, {"S": 6.0})
    assert {c.bead for c in found.charges} == {"A", "B", "C"}


def test_a_fix_to_two_cards_splits_the_fixing_beads_cost() -> None:
    commits = chain(
        commit("one", None, 0, "feat: card one", ONE),
        commit("two", "one", 1, "feat: card two", TWO),
        commit("f1", "two", 2, "fix: both cards", ONE, TWO),
        commit("f2", "f1", 3, "docs: notes", "docs/notes.md"),
    )
    history = History(commits, {"A": ["one"], "B": ["two"], "F": ["f1", "f2"]}, [])
    found = compute(history, {}, {"F": 6.0, "A": 1.0, "B": 1.0})
    # F changed three files, two of them cards it fixed: 6 * 2 / 3 = 4, two each.
    assert found.charges == [
        Charge("A", "F", CARD_FIX, 2.0),
        Charge("B", "F", CARD_FIX, 2.0),
    ]
    assert found.fixed_later == {ONE, TWO}


def test_a_beads_own_later_fix_is_not_its_rework() -> None:
    commits = chain(
        commit("one", None, 0, "feat: card one", ONE),
        commit("two", "one", 1, "feat: card two", TWO),
        commit("a2", "two", 2, "fix: card one and two", ONE, TWO, "src/x.py"),
    )
    history = History(commits, {"A": ["one", "a2"], "B": ["two"]}, [])
    found = compute(history, {}, {"A": 9.0, "B": 1.0})
    # A fixed its own card and B's among three files: only B's is rework, 9 * 1 / 3.
    assert found.charges == [Charge("B", "A", CARD_FIX, 3.0)]
    assert found.dollars("A") == 0.0
    assert found.fixed_later == {TWO}


def test_master_times_come_from_the_reflog() -> None:
    commits = chain(
        commit("a1", None, 0, "feat: a", "a"),
        commit("a2", "a1", 1, "feat: b", "b"),
        commit("a3", "a2", 2, "feat: c", "c"),
    )
    history = History(
        commits,
        {},
        [("2026-11-15T11:00:00Z", "a2"), ("2026-11-15T12:00:00Z", "a3")],
    )
    assert history.on_master == {
        "a1": "2026-11-15T11:00:00Z",
        "a2": "2026-11-15T11:00:00Z",
        "a3": "2026-11-15T12:00:00Z",
    }


CARD = "rust/cards/cards_2025/abc/card.rs"
# by-e.1 adds a card at 10:20 UTC; by-e.2 fixes it at 10:55; master reaches both at 12:00.
SNAPSHOT: bytes = git_snapshot(
    [
        ("e10", "", "2026-11-15T02:20:00-08:00", "feat: a card", [("A", CARD)]),
        ("e20", "e10", "2026-11-15T10:55:00Z", "fix: the card", [("M", CARD)]),
    ],
    [("e20", 1794744000)],  # 2026-11-15T12:00:00Z
)


def total_of(found: dict[str, object]) -> dict[str, object]:
    whole = found["total"]
    assert isinstance(whole, dict)
    return whole


def add_commits(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """Ingests the tickets again with their commits, and the commits source."""
    old = cli.sources()
    sources = [Beads(lambda: with_commits(BEADS)), *old[1:], Commits(lambda: SNAPSHOT)]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    assert cli.main(["ingest"]) == 0
    capsys.readouterr()


def with_commits(beads: bytes) -> bytes:
    records = json.loads(beads)
    for r in records:
        if r["id"] == "by-e.1":
            r["metadata"] = {**r.get("metadata", {}), "enact_commits": "e10"}
            r["started_at"] = "2026-11-15T10:00:00Z"
        if r["id"] == "by-e.2":
            r["metadata"] = {**r.get("metadata", {}), "enact_commits": "e20"}
    return json.dumps(records).encode()


def test_rework_never_changes_direct_cost(
    root: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    _, bead_before = run(capsys, "bead", "by-e.1")
    _, epic_before = run(capsys, "epic", "by-e")
    _, fixer = run(capsys, "bead", "by-e.2")
    add_commits(monkeypatch, capsys)
    shown, bead_after = run(capsys, "bead", "by-e.1")
    _, epic_after = run(capsys, "epic", "by-e")
    assert bead_after["dollars"] == bead_before["dollars"]
    assert total_of(epic_after)["dollars"] == total_of(epic_before)["dollars"]
    # by-e.2's only file is by-e.1's card, so all its cost is by-e.1's rework.
    assert bead_after["rework"] == fixer["dollars"]
    assert bead_after["rework_from"] == [
        {"fixer": "by-e.2", "rule": CARD_FIX, "dollars": fixer["dollars"]}
    ]
    assert total_of(epic_after)["rework"] == fixer["dollars"]
    assert bead_after["cards"] == 1 and bead_after["commits"] == 1
    # Created 11-01 10:00, started 11-15 10:00, closed 10:30:05; committed 10:20, on
    # master at 12:00.
    assert bead_after["queue_minutes"] == 14 * 24 * 60
    assert bead_after["work_minutes"] == 30.1
    assert bead_after["to_master_minutes"] == 100.0
    assert "rework from: by-e.2 card fix" in shown.replace("    ", " ")


@pytest.mark.parametrize(
    "query",
    [
        ["cards", "--by", "tier"],
        ["bead", "by-e.1"],
        ["summary"],
        *(
            ["metrics", m]
            for m in (
                "card.dollars",
                "card.agent-minutes",
                "bead.rework-share",
                "bead.lead-minutes",
            )
        ),
    ],
    ids=[
        "cards",
        "bead",
        "summary",
        "card.dollars",
        "card.agent-minutes",
        "bead.rework-share",
        "bead.lead-minutes",
    ],
)
def test_card_and_rework_numbers_in_the_text_are_in_the_json(
    root: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    query: list[str],
) -> None:
    add_commits(monkeypatch, capsys)
    shown, found = run(capsys, *query, "--since", "all")
    have = numbers_in(found)
    body = shown.split("\nmore: ")[0]
    seen = NUMBER.findall(body)
    assert seen, body
    for token in seen:
        assert any(abs(float(token) - n) < 1e-9 for n in have), (token, body)


def test_the_cards_query_puts_rework_beside_direct_cost(
    root: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    add_commits(monkeypatch, capsys)
    _, card = run(capsys, "bead", "by-e.1")
    _, fixer = run(capsys, "bead", "by-e.2")
    shown, found = run(capsys, "cards", "--by", "tier", "--since", "all")
    assert "direct" in shown and "rework" in shown
    groups = found["groups"]
    assert isinstance(groups, list) and [g["name"] for g in groups] == ["opus"]
    opus = groups[0]
    assert opus["cards"] == 1 and opus["beads"] == 1
    assert opus["direct"] == card["dollars"] and opus["rework"] == fixer["dollars"]
    assert opus["fixed_later"] == 100.0
    assert total_of(found)["cards"] == 1
