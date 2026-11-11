import json
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import conventions


def data(*entries: dict[str, Any]) -> dict[str, Any]:
    return {
        "title": "UI Conventions",
        "intro": ["Arena is the reference."],
        "sections": [
            {"kind": "exception", "heading": "Beryl exceptions"},
            {
                "kind": "settled",
                "heading": "Settled as Arena",
                "intro": ["Arena wins."],
            },
            {"kind": "no-equivalent", "heading": "No Arena equivalent"},
            {"kind": "not-seen", "heading": "Not seen in Arena"},
            {"kind": "open", "heading": "Still open"},
        ],
        "entries": list(entries),
    }


EXCEPTION = {
    "kind": "exception",
    "number": 1,
    "name": "Phase icons",
    "rule": "Twelve icons.",
    "reports": ["by-2o5", "by-cio"],
    "arena": {
        "does": "Arena shows one row.",
        "evidence": [{"recording": "UrGalw_f35w", "time": "48:36"}],
    },
}
SETTLED = {"kind": "settled", "name": "Tokens", "rule": "They appear at their slot."}


def test_the_page_cites_reports_and_evidence_under_its_kind() -> None:
    page = conventions.render(data(SETTLED, EXCEPTION))
    assert page == (
        "# UI Conventions\n\nArena is the reference.\n\n## Beryl exceptions\n\n"
        "1. **Phase icons.** Twelve icons (by-2o5, by-cio). Arena shows one row\n"
        "   (UrGalw_f35w 48:36).\n\n"
        "## Settled as Arena\n\nArena wins.\n\n"
        "- **Tokens.** They appear at their slot.\n"
    )


def test_bad_data_is_refused() -> None:
    renumbered = {**EXCEPTION, "number": 2}
    unnumbered = {**EXCEPTION, "number": 1, "kind": "open"}
    misfiled = {**SETTLED, "kind": "maybe", "reports": ["#12"]}
    film = {"recording": "nope", "time": "soon"}
    unfilmed = {**SETTLED, "arena": {"does": "It flies.", "evidence": [film]}}
    found = conventions.problems(data(renumbered, unnumbered, misfiled, unfilmed))
    assert len(found) == 6, found
    with_sections = data()
    with_sections["sections"].pop()
    assert conventions.problems(with_sections)


VERB = {
    "name": "fly",
    "shows": "A card travels.",
    "arena": {
        "does": "A land glides.",
        "evidence": [{"recording": "UrGalw_f35w", "time": "16:14.8"}],
    },
    "values": {"duration": None, "delay": None, "stagger": None, "curve": None},
}
MARK = {"name": "hostile-target", "means": "Not its chooser's.", "look": "Red."}


def listed(verbs: list[dict[str, Any]], marks: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        **data(),
        "lists": [
            {"key": "verbs", "heading": "Verbs", "intro": ["How things move."]},
            {"key": "marks", "heading": "Marks"},
        ],
        "verbs": verbs,
        "marks": marks,
    }


def test_the_page_lists_verbs_and_marks_after_the_rulings() -> None:
    page = conventions.render(listed([VERB], [MARK]))
    assert page == (
        "# UI Conventions\n\nArena is the reference.\n\n"
        "## Verbs\n\nHow things move.\n\n"
        "- **fly.** A card travels. A land glides (UrGalw_f35w 16:14.8).\n\n"
        "## Marks\n\n- **hostile-target.** Not its chooser's. Red.\n"
    )


def test_bad_lists_are_refused() -> None:
    assert not conventions.problems(listed([VERB], [MARK]))
    timed = {**VERB, "values": {"duration": 0.4}}
    unfilmed = {**VERB, "arena": {"does": "It flies.", "evidence": [{"time": "x"}]}}
    silent = {"name": "Hostile Target", "look": "Red."}
    found = conventions.problems(listed([VERB, timed, unfilmed], [MARK, silent]))
    # A verb listed twice (twice), values of the wrong shape, a moment that is no recording
    # and no time, a mark with no name and no meaning.
    assert len(found) == 7, found
    unheaded = listed([VERB], [MARK])
    unheaded["lists"].pop()
    assert conventions.problems(unheaded)


def test_the_check_fails_on_a_stale_page(tmp_path: Path) -> None:
    source = tmp_path / "ui-conventions.json"
    source.write_text(json.dumps(data(EXCEPTION)))
    page = tmp_path / "ui-conventions.md"
    assert conventions.stale(source, page)
    page.write_text(conventions.render(conventions.load(source)))
    assert not conventions.stale(source, page)
    source.write_text(json.dumps(data(EXCEPTION, SETTLED)))
    assert conventions.stale(source, page)
    source.write_text(json.dumps(data({**EXCEPTION, "number": 3})))
    with pytest.raises(ValueError, match="numbered"):
        conventions.stale(source, page)
