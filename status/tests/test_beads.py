"""Reading an export and indexing it: parents, children, blockers and the home page's beads."""

from __future__ import annotations

from beryl_status.beads import Index, parse

from tests.conftest import EXPORT


def index() -> Index:
    return Index.build(parse(EXPORT))


def test_parse_skips_lines_that_are_not_beads() -> None:
    assert sorted(index().beads) == [
        "by-ci",
        "by-epic",
        "by-epic.1",
        "by-epic.2",
        "by-free",
        "by-old",
    ]


def test_dependencies_become_parents_children_and_blockers() -> None:
    found = index()
    assert found.parent == {"by-epic.1": "by-epic", "by-epic.2": "by-epic"}
    assert sorted(found.children["by-epic"]) == ["by-epic.1", "by-epic.2"]
    assert found.blocked_by == {"by-epic.2": ["by-free"]}
    assert found.blocking == {"by-free": ["by-epic.2"]}


def test_top_level_is_what_just_beads_lists() -> None:
    # Open, no parent, not filed by CI; most active status first, then priority.
    assert [b.id for b in index().top_level()] == ["by-free", "by-epic"]


def test_comments_and_labels() -> None:
    found = index()
    assert [c.author for c in found.beads["by-epic.1"].comments] == ["Someone"]
    assert found.beads["by-free"].labels == ("tier:opus",)
