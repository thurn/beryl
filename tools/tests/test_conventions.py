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
