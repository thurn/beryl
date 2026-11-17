"""`just b3d match`: the target a command line names, the element records both clients write
(`web.json` and `b3d.json`), and the fonts check's warnings."""

from __future__ import annotations

import argparse
import copy

from beryl_tools.beryl3d.match import fonts, records, target


def args(recording: str, *rest: str) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    target.arguments(parser)
    return parser.parse_args([recording, "--at", "view:2", *rest])


def test_a_recording_alone_is_drawn_at_the_retina_scale_in_its_layouts_window() -> None:
    found = target.target(args("opening-play"))
    assert (found.seat, found.layout, found.size, found.scale) == (
        "you",
        "desktop",
        "1600x900",
        "2",
    )
    phone = target.target(args("opening-play", "--layout", "phone"))
    assert (phone.size, phone.layout) == ("393x759", "phone")


def test_a_tests_name_gives_seat_window_scale_and_the_layout_its_window_plays() -> None:
    retina = target.target(args("b3d:pay-default/opponent/1600x900@2"))
    assert (retina.recording, retina.seat, retina.size, retina.scale) == (
        "pay-default",
        "opponent",
        "1600x900",
        "2",
    )
    assert retina.test_name() == "b3d:pay-default/opponent/1600x900@2"
    # A test played at scale 1 names no scale, and a phone window plays the phone layout.
    phone = target.target(args("b3d:screen/start/you/393x759"))
    assert (phone.recording, phone.layout, phone.scale) == (
        "screen/start",
        "phone",
        "1",
    )
    assert phone.test_name() == "b3d:screen/start/you/393x759"


def test_two_targets_that_differ_only_in_moment_or_scale_have_their_own_snapshots() -> (
    None
):
    a = target.target(args("opening-play"))
    b = target.target(args("opening-play", "--scale", "1"))
    c = target.target(
        argparse.Namespace(**{**vars(args("opening-play")), "at": "1500ms"})
    )
    assert len({a.slug(), b.slug(), c.slug()}) == 3
    assert "/" not in target.target(args("b3d:screen/start/you/393x759")).slug()


def element(**given: object) -> dict[str, object]:
    style: dict[str, object] = {
        "background-color": [0, 0, 0, 0],
        "background-image": "none",
        "border-width": 0,
        "border-color": [0, 0, 0, 0],
        "border-radius": 0,
        "box-shadow": "none",
        "filter": "none",
        "opacity": 1,
        "color": [245, 243, 240, 1],
        "font-family": "Open Sans",
        "font-size": 16,
        "font-weight": 400,
        "mask-image": "none",
        "transform": "none",
    }
    base: dict[str, object] = {
        "key": 'button "Play" #65554',
        "role": "button",
        "name": "Play",
        "object": None,
        "box": [1151, 749, 192, 48],
        "z": 0,
        "parent": None,
        "style": style,
        "text": [
            {
                "string": "Play",
                "family": "Crimson Pro",
                "size": 20,
                "weight": 600,
                "color": [20, 18, 16, 1],
                "baseline": 779.5,
                "box": [1229, 761, 36, 24],
            }
        ],
    }
    return {**base, **given}


def document(*elements: dict[str, object]) -> dict[str, object]:
    return {
        "target": "b3d:opening-play/you/1600x900@2",
        "scale": 2.0,
        "elements": list(elements),
    }


def test_records_that_keep_the_contract_have_no_faults() -> None:
    # A card drawn on the table has no text of its own, and Beryl 3D writes no type for it.
    card = element(role="card", key="card #38", object=38, parent=0, z=1, text=[])
    style = card["style"]
    assert isinstance(style, dict)
    card["style"] = {k: v for k, v in style.items() if k not in records.TYPE}
    assert records.faults(document(element(), card)) == []


def test_each_way_a_record_breaks_the_contract_is_named() -> None:
    bad = element(box=[1, 2, 3], parent=4, z=1.5)
    style = copy.deepcopy(bad["style"])
    assert isinstance(style, dict)
    del style["box-shadow"]
    style["color"] = "white"
    style["outline"] = "none"
    bad["style"] = style
    found = records.faults(document(bad))
    assert found == [
        "element 0: box [1, 2, 3] is not [x, y, width, height]",
        "element 0: z 1.5 is not a whole number",
        "element 0: parent 4 is not an earlier element",
        "element 0: unknown style 'outline'",
        "element 0: style lacks 'box-shadow'",
        "element 0: color 'white' is not [r, g, b, a]",
    ]
    assert records.faults({"elements": []}) == ["lacks 'target'", "lacks 'scale'"]


def test_a_line_without_its_baseline_or_with_an_alpha_over_one_is_a_fault() -> None:
    line = {
        "string": "Draw",
        "family": "Crimson Pro",
        "size": 20,
        "weight": 600,
        "color": [20, 18, 16, 255],
        "box": [0, 0, 1, 1],
    }
    assert records.faults(document(element(text=[line]))) == [
        "element 0: line lacks 'baseline'",
        "element 0: line color [20, 18, 16, 255] is not [r, g, b, a]",
    ]


def test_fonts_warn_of_a_different_version_and_of_a_file_that_was_not_read() -> None:
    read = {
        str(fonts.BUNDLED / "Cinzel.ttf"): {"family": "Cinzel", "version": "2.000"},
        str(fonts.FONTSOURCE / fonts.PAIRS[0][1]): {
            "family": "Cinzel",
            "version": "2.001",
        },
        str(fonts.BUNDLED / "CrimsonPro.ttf"): {
            "family": "Crimson Pro",
            "version": "1",
        },
        str(fonts.FONTSOURCE / fonts.PAIRS[1][1]): {
            "family": "Crimson Pro",
            "version": "1",
        },
        str(fonts.BUNDLED / "OpenSans.ttf"): {"error": "no name table"},
    }
    assert fonts.differences(read) == [
        "warning: fonts: Cinzel.ttf has version '2.000' but the web's "
        "cinzel/files/cinzel-latin-wght-normal.woff2 has '2.001'",
        "warning: fonts: Beryl 3D's OpenSans.ttf: no name table",
        "warning: fonts: web's open-sans/files/open-sans-latin-wght-normal.woff2: not read",
    ]
