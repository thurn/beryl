"""`just b3d match census`: how each element, difference and differing pixel of a moment is
given to a screen area, and how the moments are spread across a recording."""

from __future__ import annotations

from pathlib import Path

import pytest

from beryl_tools.beryl3d.match import census

STYLE: dict[str, object] = {
    "background-color": [0, 0, 0, 0],
    "background-image": "none",
    "border-width": 0,
    "border-color": [0, 0, 0, 0],
    "border-radius": 0,
    "box-shadow": "none",
    "filter": "none",
    "opacity": 1,
    "mask-image": "none",
    "transform": "none",
}


def record(
    role: str,
    name: str,
    box: list[float],
    parent: int | None = None,
    place: str = "",
    **style: object,
) -> dict[str, object]:
    return {
        "key": None,
        "role": role,
        "name": name,
        "object": None,
        "place": place,
        "box": box,
        "z": 0,
        "parent": parent,
        "style": {**STYLE, **style},
        "text": [],
    }


# Beryl 3D's side of a moment: the flat layer's root holding the prompt and its button, a card
# in hand with its mana cost printed on it, a land, a library, the opponent's steps and an
# unnamed icon no area claims.
B3D: list[dict[str, object]] = [
    record("region", "game", [1300, 700, 260, 110]),
    record("region", "prompt", [1300, 700, 260, 110], parent=0),
    record("button", "Next", [1316, 716, 192, 48], parent=1),
    record("card", "Island", [623, 726, 188, 258], place="hand.yours"),
    record("text", "{1}{W}", [745, 707, 52, 25], parent=3, place="hand.yours"),
    record("card", "Plains", [251, 572, 118, 133], place="battlefield.yours.lands"),
    record("pile", "Your library", [33, 795, 82, 91], place="library.yours"),
    record("region", "Opponent's steps", [645, 80, 311, 50]),
    record("icon", "", [1200, 400, 22, 22]),
]
# The web's side: the page's root across the whole window, the same prompt with its button
# moved and recolored and a label Beryl 3D lacks, the card in hand, a small unnamed box under
# nothing paired, and a line of text inside the root, which is not the cloth.
WEB: list[dict[str, object]] = [
    record("region", "", [0, 0, 1600, 900]),
    record("region", "prompt", [1300, 700, 260, 110], parent=0),
    record(
        "button",
        "Next",
        [1316, 720, 192, 48],
        parent=1,
        **{"background-color": [200, 40, 40, 1]},
    ),
    record("text", "To Combat", [1373, 772, 78, 22], parent=2),
    record("card", "Island", [623, 726, 188, 258]),
    record("region", "", [10, 10, 5, 5]),
    record("text", "Loading", [400, 400, 50, 20], parent=0),
]
WINDOW = 1600.0 * 900.0


def test_each_beryl_3d_element_takes_the_first_area_it_or_its_nearest_ancestor_matches() -> (
    None
):
    assert census.b3d_areas(B3D) == [
        "cloth",
        "prompt",
        "prompt",
        "hand",
        "card face",
        "battlefield",
        "piles",
        "steps",
        "other",
    ]


def test_differences_and_pixels_go_to_the_area_of_their_beryl_3d_element_else_the_webs() -> (
    None
):
    pixels = {
        "nobody": 11,
        "rows": [
            # The card in hand, drawn differently on both sides.
            {"web": 4, "b3d": 3, "b3d_pixels": 100, "b3d_bare": 0},
            # The web's label, where no Beryl 3D element paints: the prompt's.
            {"web": 3, "b3d": None, "b3d_pixels": 0, "b3d_bare": 7},
            # The mana cost, where the web's topmost element is its card's row, counted once.
            {"web": None, "b3d": 4, "b3d_pixels": 5, "b3d_bare": 0},
            # The web page's root over the cloth, where Beryl 3D paints nothing.
            {"web": 0, "b3d": None, "b3d_pixels": 0, "b3d_bare": 20},
        ],
    }
    found = census.tally(WEB, B3D, pixels, WINDOW)

    def counts(**given: int) -> dict[str, int]:
        return {**dict.fromkeys(census.COUNTS, 0), **given}

    assert found == {
        # The button moved by 4 points and has a different background; the label is the web's
        # alone.
        "prompt": counts(pixels=7, boxes=1, styles=1, unmatched=1),
        "hand": counts(pixels=100),
        "card face": counts(pixels=5, unmatched=1),
        # The web page's root and the pixels no side paints. The flat layer's root and the
        # page's root are wrappers that draw nothing, so the matcher leaves them unmatched
        # without counting them.
        "cloth": counts(pixels=20 + 11),
        "battlefield": counts(unmatched=1),
        "piles": counts(unmatched=1),
        # The web's text and Beryl 3D's unnamed icon. The opponent's steps region and the
        # web's small unnamed box draw nothing, so neither counts.
        "other": counts(unmatched=2),
    }


def test_the_moments_are_spread_across_the_recording_and_each_taken_once() -> None:
    assert census.spread([0, 90, 180], 8) == [0, 90, 180]
    times = [0, 1, 2, 3, 50, 51, 52, 100]
    assert census.spread(times, 4) == [3, 50, 52, 100]
    assert len(census.spread(list(range(1000)), 8)) == 8


def test_moments_added_by_hand_are_measured_with_the_chosen_ones(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    moments = tmp_path / "census.json"
    moments.write_text(
        '{"about": "", "recordings": {"a": [10, 20], "b": [5]},'
        ' "added": {"a": [30, 10], "c": [7]}}'
    )
    monkeypatch.setattr(census, "MOMENTS", moments)
    assert census.load_moments() == {"a": [10, 20, 30], "b": [5], "c": [7]}
