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


VERB: dict[str, Any] = {
    "name": "fly",
    "shows": "A card travels.",
    "arena": {
        "does": "A land glides.",
        "evidence": [{"recording": "UrGalw_f35w", "time": "16:14.8"}],
    },
    "values": {"duration": None, "delay": None, "stagger": None, "curve": None},
}
MARK: dict[str, Any] = {
    "name": "hostile-target",
    "means": "Not its chooser's.",
    "look": "Red.",
}


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
    assert conventions.stale(source, page, None)
    page.write_text(conventions.render(conventions.load(source, None)))
    assert not conventions.stale(source, page, None)
    source.write_text(json.dumps(data(EXCEPTION, SETTLED)))
    assert conventions.stale(source, page, None)
    source.write_text(json.dumps(data({**EXCEPTION, "number": 3})))
    with pytest.raises(ValueError, match="numbered"):
        conventions.stale(source, page, None)


def taken(recording: str, duration: int, **more: Any) -> dict[str, Any]:
    """An instance of a verb in Arena, measured by the tracker."""
    return {
        "case": "land played",
        "recording": recording,
        "time": "16:14.8",
        "span": 1.2,
        "crop": [700, 0, 1220, 880],
        "method": "tracker",
        "why": "",
        "command": f"just video measure {recording} 16:14.8 --span 1.2",
        "since": "",
        "duration_ms": duration,
        "delay_ms": None,
        "stagger_ms": None,
        "peak": 1.0,
        "path": "a glide right",
        "curve": "ease-out",
        "note": "",
        **more,
    }


UNSEEN = {"kind": "not-seen", "name": "A card turning face up", "rule": "It flips."}
FLIP: dict[str, Any] = {**VERB, "name": "flip", "shows": "A card turns over."}


def words(conventions_data: dict[str, Any]) -> str:
    """The rendered page, unwrapped."""
    return " ".join(conventions.render(conventions_data).split())


def measuring(*instances: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    """Conventions listing fly and flip, and Arena's measurements of fly."""
    conventions_data = {**listed([{**VERB}, {**FLIP}], [MARK]), "entries": [UNSEEN]}
    motion = {
        "about": ["How they were taken."],
        "verbs": [
            {"verb": "fly", "path": "a glide", "instances": list(instances)},
            {"verb": "flip", "unseen": "Nothing flips.", "not-seen": UNSEEN["name"]},
        ],
    }
    return conventions_data, motion


def test_a_verbs_values_are_the_medians_of_its_instances() -> None:
    drawn = taken("SswBO8aOOgc", 900, case="draw", curve="ease-in", stagger_ms=330)
    conventions_data, motion = measuring(
        taken("UrGalw_f35w", 600), taken("UrGalw_f35w", 500, peak=1.2), drawn
    )
    conventions.derive(motion, conventions_data)
    assert not conventions.motion_problems(motion, conventions_data)
    summary = motion["verbs"][0]["summary"]
    assert summary["duration_ms"] == {"median": 600, "min": 500, "max": 900}
    assert summary["peak"] == {"median": 1.0, "min": 1.0, "max": 1.2}
    assert summary["cases"]["land played"]["duration_ms"]["median"] == 550
    fly, flip = conventions_data["verbs"]
    assert fly["values"] == {
        "duration": 600,
        "delay": None,
        "stagger": 330,
        "curve": "ease-out",
    }
    assert flip["values"] == VERB["values"]
    assert "Measured: 600 ms, 330 ms apart, ease-out." in words(conventions_data)


def test_an_exempt_verb_keeps_its_own_values() -> None:
    conventions_data, motion = measuring(
        taken("UrGalw_f35w", 600), taken("UrGalw_f35w", 500), taken("SswBO8aOOgc", 9)
    )
    conventions_data["entries"].append(EXCEPTION)
    conventions_data["verbs"][0]["exempt"] = 1
    conventions.derive(motion, conventions_data)
    assert conventions_data["verbs"][0]["values"] == VERB["values"]
    assert not conventions.problems(conventions_data)
    assert not conventions.motion_problems(motion, conventions_data)
    assert "Exempt: exception 1." in words(conventions_data)
    conventions_data["verbs"][0]["exempt"] = 2
    assert conventions.problems(conventions_data)


def test_thin_or_unexplained_measurements_are_refused() -> None:
    conventions_data, motion = measuring(
        taken("UrGalw_f35w", 600), taken("SswBO8aOOgc", 500)
    )
    conventions.derive(motion, conventions_data)
    # Two instances are thin evidence.
    assert len(conventions.motion_problems(motion, conventions_data)) == 1
    # Three in one recording are one recording's habit.
    one_recording, motion = measuring(*[taken("UrGalw_f35w", 600)] * 3)
    conventions.derive(motion, one_recording)
    assert len(conventions.motion_problems(motion, one_recording)) == 1

    unexplained = taken("UrGalw_f35w", 600, method="filmstrip", delay_ms=40)
    untimed = taken("SswBO8aOOgc", 600, duration_ms=None, crop=[1, 2], curve="bouncy")
    conventions_data, motion = measuring(unexplained, untimed, taken("pE4XSNi-nZc", 1))
    motion["verbs"][1]["not-seen"] = "Nothing"
    found = conventions.motion_problems(motion, conventions_data)
    # No why, a delay from nothing, no duration, a bad crop and curve, a dangling not-seen.
    assert len(found) == 6, found


def test_stale_summaries_and_values_are_refused() -> None:
    conventions_data, motion = measuring(
        taken("UrGalw_f35w", 600), taken("UrGalw_f35w", 500), taken("SswBO8aOOgc", 9)
    )
    conventions.derive(motion, conventions_data)
    motion["verbs"][0]["instances"][0]["duration_ms"] = 400
    assert len(conventions.motion_problems(motion, conventions_data)) == 2
    motion["verbs"].reverse()
    assert any(
        "want ['fly', 'flip']" in p
        for p in conventions.motion_problems(motion, conventions_data)
    )
