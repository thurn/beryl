"""Reading an export and indexing it: parents, children, blockers and the home page's beads."""

from __future__ import annotations

from beryl_status.beads import Bead, Index, parse

from tests.conftest import EXPORT


def index() -> Index:
    return Index.build(parse(EXPORT))


def test_parse_skips_lines_that_are_not_beads() -> None:
    assert sorted(index().beads) == [
        "by-bug",
        "by-ci",
        "by-epic",
        "by-epic.1",
        "by-epic.2",
        "by-free",
        "by-later",
        "by-old",
    ]


def test_dependencies_become_parents_children_and_blockers() -> None:
    found = index()
    assert found.parent == {
        "by-epic.1": "by-epic",
        "by-epic.2": "by-epic",
        "by-ci": "by-epic",
    }
    assert sorted(found.children["by-epic"]) == ["by-ci", "by-epic.1", "by-epic.2"]
    assert found.blocked_by == {"by-epic.2": ["by-free"]}
    assert found.blocking == {"by-free": ["by-epic.2"]}


def ids(beads: list[Bead]) -> list[str]:
    return [b.id for b in beads]


def test_home_sorts_open_beads_into_categories() -> None:
    home = index().home()
    assert ids(home.epics) == ["by-epic"]
    # Outside any epic and filed by a person; children of the epic show on its page.
    assert ids(home.beads) == ["by-free"]
    # An automatic bead shows in its group even inside an epic.
    assert [(name, ids(beads)) for name, beads in home.automatic] == [
        ("Flaky checks", ["by-ci"])
    ]
    assert ids(home.bug_reports) == ["by-bug"]
    assert ids(home.deferred) == ["by-later"]


def test_comments_and_labels() -> None:
    found = index()
    assert [c.author for c in found.beads["by-epic.1"].comments] == ["Someone"]
    assert found.beads["by-free"].labels == ("tier:opus",)
