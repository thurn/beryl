"""The card-text check: each kind of mismatch it reports, and what passes."""

from beryl_tools import card_text
from beryl_tools.card_store import Json
from beryl_tools.render_diff import Diff

MATCH = Diff(lines=[], causes=[])
WORDING = Diff(lines=["Card: draw [-a-] {+one+} card ."], causes=[])


def entry(reason: str = "wording", diff: Diff = WORDING) -> dict[str, object]:
    return {"reason": reason, "note": "the same instruction", "diff": diff.lines}


def test_a_match_or_a_pinned_difference_passes() -> None:
    diffs = {"Card": WORDING, "Other": MATCH}
    assert card_text.problems(diffs, {"Card": entry()}, {}) == []


def test_a_difference_nothing_pins_fails_and_prints_it() -> None:
    (problem,) = card_text.problems({"Card": WORDING}, {}, {})
    assert "Card differs from its Oracle text" in problem
    assert "draw [-a-] {+one+} card" in problem


def test_a_changed_difference_fails() -> None:
    changed = Diff(lines=["Card: something else"], causes=[])
    (problem,) = card_text.problems({"Card": changed}, {"Card": entry()}, {})
    assert "other than its entry pins" in problem


def test_an_entry_for_a_matching_or_missing_card_fails() -> None:
    found = card_text.problems({"Card": MATCH}, {"Card": entry(), "Gone": entry()}, {})
    assert found == [
        "Card now matches its Oracle text: drop its entry",
        "Gone has an entry but no card file: drop its entry",
    ]


def test_an_escalated_entry_needs_a_partial_escalation() -> None:
    pinned: Json = {"Card": entry("escalated")}
    (problem,) = card_text.problems({"Card": WORDING}, pinned, {})
    assert "no partial entry" in problem
    partial: Json = {"Card": {"mechanic": "regeneration", "partial": True}}
    assert card_text.problems({"Card": WORDING}, pinned, partial) == []


def test_an_entry_needs_a_known_reason_and_a_note() -> None:
    bad: Json = {"Card": {"reason": "close enough", "note": " ", "diff": WORDING.lines}}
    assert card_text.problems({"Card": WORDING}, bad, {}) == [
        "Card: reason must be one of wording, escalated",
        "Card: say in `note` why the texts differ",
    ]
