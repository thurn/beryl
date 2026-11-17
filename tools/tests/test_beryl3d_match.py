"""`just b3d match`: the target a command line names, the element records both clients write
(`web.json` and `b3d.json`), the fonts check's warnings, the matcher and box diff that
`match boxes` prints, and how `--only` finds the web element it draws alone."""

from __future__ import annotations

import argparse
import copy
import json
from dataclasses import replace
from pathlib import Path

from beryl_tools.beryl3d.match import boxes, fonts, joined, records, snapshot, target


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


def line(string: str, at: list[float]) -> dict[str, object]:
    return {
        "string": string,
        "family": "Open Sans",
        "size": 16,
        "weight": 400,
        "color": [245, 243, 240, 1],
        "baseline": at[1] + at[3] - 4,
        "box": at,
    }


def plain(role: str, name: str, at: list[float], **given: object) -> dict[str, object]:
    """An element with no key and no text unless `given` says otherwise."""
    return element(
        **{"role": role, "name": name, "key": None, "box": at, "text": [], **given}
    )


def test_the_matcher_pairs_by_key_then_role_name_and_parent_then_nearest_box() -> None:
    web = [
        # 0: the same locator on both sides.
        element(key='button "Play" #7'),
        # 1, 2: no key on the web; the prompt pairs by name, and the two labels named alike
        # pair under it, not under the other region.
        plain("region", "prompt", [980, 733, 580, 80]),
        plain("text", "Pay", [996, 749, 144, 48], parent=1),
        plain("region", "log", [0, 0, 300, 900]),
        plain("text", "Pay", [10, 10, 40, 20], parent=3),
        # 5: unnamed, so only its box can pair it: 3 points away pairs, 30 does not.
        plain("icon", "", [29, 29, 22, 22]),
        plain("icon", "", [100, 100, 22, 22]),
    ]
    b3d = [
        element(key='button "Play" #7', box=[1150, 749, 192, 48]),
        plain("region", "log", [0, 0, 300, 900], key='region "log" #3'),
        plain("text", "Pay", [12, 12, 40, 20], parent=1, key='text "Pay" #4'),
        plain("region", "prompt", [980, 733, 580, 80], key='region "prompt" #5'),
        plain("text", "Pay", [12, 12, 40, 20], parent=3, key='text "Pay" #6'),
        plain("icon", "", [32, 29, 22, 22]),
        plain("icon", "", [130, 100, 22, 22]),
    ]
    found = joined.join(web, b3d)
    listed = found["pairs"]
    assert isinstance(listed, list)
    pairs = {(p["web"], p["b3d"]): p["pass"] for p in listed}
    assert pairs == {
        (0, 0): "key",
        (1, 3): "identity",
        (2, 4): "identity",
        (3, 1): "identity",
        (4, 2): "identity",
        (5, 5): "box",
    }
    unmatched = found["unmatched"]
    assert isinstance(unmatched, dict)
    assert [e["index"] for e in unmatched["web"]] == [6]
    assert [e["box"] for e in unmatched["b3d"]] == [[130, 100, 22, 22]]


def test_boxes_lists_moved_and_rewrapped_elements_worst_first_and_text_drawn_over_text() -> (
    None
):
    web = [
        plain(
            "text",
            "flip",
            [600, 158, 390, 59],
            text=[line("You won", [600, 158, 390, 29])],
        ),
        plain("button", "Settings", [1542, 14, 44, 44]),
        plain("button", "Report", [1542, 70, 44, 44]),
        plain("text", "life", [780, 82, 38, 46], text=[line("20", [780, 82, 38, 46])]),
        plain(
            "text",
            "play",
            [600, 230, 200, 20],
            text=[line("on the play", [600, 230, 200, 20])],
        ),
    ]
    b3d = copy.deepcopy(web)
    # The heading wraps onto two lines, a button moves 6 points, another a quarter point, and
    # the life total slides down over "on the play".
    b3d[0]["text"] = [
        line("You", [600, 158, 390, 29]),
        line("won", [600, 187, 390, 29]),
    ]
    b3d[1]["box"] = [1548, 14, 44, 44]
    b3d[2]["box"] = [1542.25, 70, 44, 44]
    b3d[3]["box"] = [780, 220, 38, 46]
    b3d[3]["text"] = [line("20", [780, 220, 38, 46])]
    pairs = joined.join(web, b3d)["pairs"]
    assert isinstance(pairs, list)
    found = boxes.differences(web, b3d, pairs)
    assert [d.line() for d in found] == [
        'text "flip"  web [600 158 390 59]  b3d [600 158 390 59]  dx 0 dy 0 dw 0 dh 0  '
        "WRAP 1/2",
        'text "life"  web [780 82 38 46]  b3d [780 220 38 46]  dx 0 dy +138 dw 0 dh 0',
        'button "Settings"  web [1542 14 44 44]  b3d [1548 14 44 44]  dx +6 dy 0 dw 0 dh 0',
    ]
    only_b3d, only_web = boxes.one_sided_overlaps(web, b3d, pairs)
    assert only_b3d == ['text "life"  and  text "play"']
    assert only_web == []


def test_a_label_on_a_child_counts_as_its_buttons_text() -> None:
    # The web puts a button's label on a span inside it; Beryl 3D on the button itself.
    web = [
        plain("button", "Play", [820, 287, 250, 48]),
        plain(
            "text",
            "",
            [922, 299, 45, 24],
            parent=0,
            text=[line("Play", [922, 299, 45, 24])],
        ),
    ]
    b3d = [element(box=[820, 287, 250, 48], key='button "Play" #1')]
    b3d[0]["text"] = [line("Play", [922, 299, 45, 24])]
    pairs = joined.join(web, b3d)["pairs"]
    assert isinstance(pairs, list)
    assert boxes.differences(web, b3d, pairs) == []


def test_a_target_drawn_with_only_has_a_snapshot_of_its_own_and_the_same_test_name() -> (
    None
):
    parser = argparse.ArgumentParser()
    target.arguments(parser)
    target.only_argument(parser)
    whole = target.target(parser.parse_args(["opening-play", "--at", "prompt:1"]))
    only = target.target(
        parser.parse_args(
            ["opening-play", "--at", "prompt:1", "--only", 'button "Play"']
        )
    )
    assert only.only == 'button "Play"'
    assert replace(only, only=None) == whole
    assert only.slug() != whole.slug() and only.slug().startswith(whole.slug())
    assert '"' not in only.slug() and " " not in only.slug()
    assert only.test_name() == whole.test_name()


def bare(key: str | None, role: str, name: str, box: list[float]) -> dict[str, object]:
    return {
        "key": key,
        "role": role,
        "name": name,
        "object": None,
        "box": box,
        "parent": None,
    }


def test_only_finds_the_web_pair_of_the_element_its_locator_names(
    tmp_path: Path,
) -> None:
    b3d = [
        bare('button "Play" #5', "button", "Play", [0, 0, 100, 40]),
        bare('button "Play later" #6', "button", "Play later", [0, 50, 100, 40]),
        bare('text "Alone" #7', "text", "Alone", [500, 500, 10, 10]),
    ]
    web = [
        bare(None, "button", "Play later", [0, 50, 100, 40]),
        bare(None, "button", "Play", [0, 0, 100, 40]),
    ]
    for side, elements in (("b3d", b3d), ("web", web)):
        (tmp_path / f"{side}.json").write_text(
            json.dumps({"target": "b3d:x/you/1600x900@2", "elements": elements})
        )
    # A locator equal to a key wins over the longer key it also starts.
    assert snapshot.web_pair(tmp_path, 'button "Play" #5') == (1, "")
    assert snapshot.web_pair(tmp_path, 'button "Play later"') == (0, "")
    index, why = snapshot.web_pair(tmp_path, 'button "Play')
    assert index is None and "names 2 Beryl 3D elements" in why
    index, why = snapshot.web_pair(tmp_path, 'text "Alone"')
    assert index is None and "no web element is paired" in why
