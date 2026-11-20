"""`just b3d match styles` and `gaps`: parsing WebKit's computed shadows, gradients and filters
(css.py), which values Beryl 3D can express (support.py), the style diff and the gaps list.
"""

from __future__ import annotations

import argparse

from beryl_tools.beryl3d.match import css, gaps, styles, support, target


def test_a_box_shadow_of_several_layers_keeps_each_layer_and_which_are_inset() -> None:
    parsed = css.parsed(
        "box-shadow",
        "rgba(255, 240, 190, 0.7) 0px 1px 1px 0px inset, rgba(60, 25, 5, 0.85) 0px 0px 0px "
        "1px, rgba(0, 0, 0, 0.35) 0px 4px 8px 0px",
    )
    assert parsed == [
        {
            "inset": True,
            "x": 0.0,
            "y": 1.0,
            "blur": 1.0,
            "spread": 0.0,
            "color": [255, 240, 190, 0.7],
        },
        {
            "inset": False,
            "x": 0.0,
            "y": 0.0,
            "blur": 0.0,
            "spread": 1.0,
            "color": [60, 25, 5, 0.85],
        },
        {
            "inset": False,
            "x": 0.0,
            "y": 4.0,
            "blur": 8.0,
            "spread": 0.0,
            "color": [0, 0, 0, 0.35],
        },
    ]
    assert support.shape("box-shadow", parsed) == "3 layers, 1 inset"
    # The color may come last, as an author writes it.
    last = css.parsed("box-shadow", "2px 3px 4px rgb(1, 2, 3)")
    assert isinstance(last, list) and last[0]["color"] == [1, 2, 3, 1.0]
    assert css.parsed("box-shadow", "none") == "none"


def test_a_gradient_keeps_its_stops_hints_and_angle_with_sides_as_degrees() -> None:
    parsed = css.parsed(
        "background-image",
        "linear-gradient(rgba(1, 2, 3, 0.5) 0%, rgb(4, 5, 6) 40% 60%, 70%, "
        'color(srgb 1 0 0) 100%), url("felt.png")',
    )
    assert parsed == [
        {
            "kind": "linear",
            "repeating": False,
            # No angle runs top to bottom.
            "shape": 180.0,
            "stops": [
                {"color": [1, 2, 3, 0.5], "at": ["0%"]},
                {"color": [4, 5, 6, 1.0], "at": ["40%", "60%"]},
                # A color hint: a position with no color.
                {"color": None, "at": ["70%"]},
                {"color": [255, 0, 0, 1.0], "at": ["100%"]},
            ],
        },
        {"kind": "url", "url": "felt.png"},
    ]
    assert support.shape("background-image", parsed) == "linear + url"
    right = css.parsed(
        "background-image", "linear-gradient(to right, rgb(1, 2, 3), rgb(4, 5, 6))"
    )
    angle = css.parsed(
        "background-image", "linear-gradient(90deg, rgb(1, 2, 3), rgb(4, 5, 6))"
    )
    assert right == angle
    repeating = css.parsed(
        "background-image",
        "repeating-linear-gradient(45deg, rgb(1, 2, 3) 0px, rgb(4, 5, 6) 10px)",
    )
    assert isinstance(repeating, list)
    assert repeating[0]["repeating"] and repeating[0]["stops"][1]["at"] == [10.0]
    assert support.shape("background-image", repeating) == "repeating-linear"


def test_a_radial_gradient_keeps_its_shape_and_a_none_layer_is_dropped() -> None:
    parsed = css.parsed(
        "mask-image",
        "radial-gradient(circle at 50%   30%, rgb(1, 2, 3) 0%, rgba(0, 0, 0, 0) 70%), none",
    )
    assert isinstance(parsed, list) and len(parsed) == 1
    assert parsed[0]["shape"] == "circle at 50% 30%"
    assert css.text(parsed) == (
        "radial-gradient(circle at 50% 30%, rgba(1,2,3,1) 0%, rgba(0,0,0,0) 70%)"
    )
    assert css.parsed("mask-image", "none, none") == "none"


def test_a_filter_chain_keeps_each_drop_shadow_and_amounts_as_numbers() -> None:
    parsed = css.parsed(
        "filter",
        "drop-shadow(rgba(255, 170, 50, 1) 0px 0px 1.5px) drop-shadow(rgb(255, 130, 20) "
        "0px 0px 3px) brightness(80%) grayscale(1) blur(4px) hue-rotate(90deg)",
    )
    assert isinstance(parsed, list)
    assert [f["fn"] for f in parsed] == [
        "drop-shadow",
        "drop-shadow",
        "brightness",
        "grayscale",
        "blur",
        "hue-rotate",
    ]
    assert parsed[0]["shadow"] == {
        "inset": False,
        "x": 0.0,
        "y": 0.0,
        "blur": 1.5,
        "spread": 0.0,
        "color": [255, 170, 50, 1.0],
    }
    assert [f["value"] for f in parsed[2:]] == [0.8, 1.0, 4.0, 90.0]
    assert (
        support.shape("filter", parsed)
        == "drop-shadow + drop-shadow + brightness + grayscale + blur + hue-rotate"
    )


def test_values_are_the_same_within_a_level_a_quarter_point_or_half_a_percent() -> None:
    assert support.same([10, 20, 30, 0.5], [11, 19, 30, 0.503], "color")
    assert not support.same([10, 20, 30, 0.5], [12, 20, 30, 0.5], "color")
    assert support.same(8, 8.2, "length") and not support.same(8, 8.3, "length")
    a = css.parsed("box-shadow", "rgb(0, 0, 0) 0px 2px 4px 0px")
    b = css.parsed("box-shadow", "rgb(1, 0, 0) 0px 2.2px 4px 0px")
    c = css.parsed("box-shadow", "rgb(0, 0, 0) 0px 2px 4px 0px inset")
    assert support.same(a, b, "structure") and not support.same(a, c, "structure")
    # A transform is compared by its rotation alone: the boxes compare position and size.
    assert support.same([1, 0, 0, 1, 10, 0], [0.5, 0, 0, 0.5, 90, 0], "transform")
    assert support.same("none", [1, 0, 0, 1, 5, 5], "transform")
    turned = [0.99, 0.13, -0.13, 0.99, 1014, 733]
    assert support.same(
        turned, [0.9914, 0.1305, -0.1305, 0.9914, 1020, 730], "transform"
    )
    assert not support.same(turned, "none", "transform")


def test_only_the_shapes_the_supported_table_lists_can_be_expressed() -> None:
    assert support.can_express("border-radius", 12)
    assert not support.can_express("border-radius", [12, 12, 0, 0])
    assert support.can_express("box-shadow", "none")
    # Shadows are drawn, and 2D transforms compared by their rotation; 3D ones are not.
    assert support.can_express("box-shadow", css.parsed("box-shadow", "0px 1px red"))
    assert support.can_express("transform", [1, 0, 0, 1, 5, 5])
    assert not support.can_express("transform", [1.0] * 16)
    assert support.can_express("border-width", [1, 2, 1, 1])
    assert support.shape("transform", [1, 0, 0, 1, 5, 5]) == "translate"
    assert support.shape("transform", [0.5, 0, 0, 0.5, 0, 0]) == "scale"
    assert support.shape("transform", [0.4, 0.1, -0.1, 0.4, 0, 0]) == "rotate or skew"
    assert support.shape("transform", [1.0] * 16) == "3d"


def style(**given: object) -> dict[str, object]:
    base: dict[str, object] = {
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
    return {**base, **{k.replace("_", "-"): v for k, v in given.items()}}


def element(
    name: str, box: list[float], text: bool = False, **given: object
) -> dict[str, object]:
    return {
        "key": f'button "{name}" #1',
        "role": "button",
        "name": name,
        "object": None,
        "box": box,
        "z": 0,
        "parent": None,
        "style": style(**given),
        "text": [{"string": name}] if text else [],
    }


def test_the_style_diff_lists_differences_and_counts_what_style_cannot_express() -> (
    None
):
    turned = [0.97, 0.26, -0.26, 0.97, 5, 5]
    web = [
        element(
            "Play",
            [0, 0, 200, 48],
            text=True,
            background_color=[180, 94, 18, 1],
            transform=turned,
            # The web computes a border color even where no border is drawn.
            border_color=[245, 243, 240, 1],
            font_family="Times New Roman",
        ),
        # No text: its type is not compared.
        element("Back", [0, 60, 40, 40], color=[1, 2, 3, 1]),
    ]
    b3d = [
        element(
            "Play",
            [0, 0, 200, 48],
            text=True,
            background_color=[181, 95, 18, 1],
            font_family="Crimson Pro",
        ),
        element("Back", [0, 60, 40, 40], background_color=[9, 9, 9, 1]),
    ]
    pairs: list[dict[str, object]] = [
        {"web": 0, "b3d": 0, "pass": "key"},
        {"web": 1, "b3d": 1, "pass": "key"},
    ]
    found = styles.measure(web, b3d, pairs)
    assert [d.line() for d in found.differences] == [
        'button "Play" #1  font-family  Times New Roman  Crimson Pro',
        'button "Play" #1  transform  0.97 0.26 -0.26 0.97 5 5  none',
        'button "Back" #1  background-color  rgba(0,0,0,0)  rgba(9,9,9,1)',
    ]
    assert found.unsupported == {}
    assert found.total() == "styles 3, unsupported 0"


def test_gaps_group_values_by_shape_and_rank_them_by_area_on_screen() -> None:
    parser = argparse.ArgumentParser()
    target.arguments(parser)
    t = target.target(parser.parse_args(["opening-play", "--at", "end"]))
    # A 3D matrix, which `Style` cannot express.
    move = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 5, 0, 1]
    radial = [{"kind": "radial", "repeating": False, "shape": None, "stops": []}]
    web = [
        element("Small", [0, 0, 10, 10], transform=move),
        element("Large", [0, 0, 100, 100], mask_image=radial),
        # Half of it lies left of the window.
        element("Edge", [-50, 0, 100, 10], transform=move),
        # Transparent: no area.
        element("Gone", [0, 0, 500, 500], transform=move, opacity=0),
        element("Corners", [0, 0, 20, 20], border_radius=[4, 4, 0, 0]),
        element("Flat", [0, 0, 20, 20], border_radius=4, background_color=[1, 1, 1, 1]),
    ]
    groups = gaps.grouped(gaps.uses(t, web))
    assert [(g.name, g.shape, len(g.uses), g.area()) for g in groups] == [
        ("mask-image", "radial", 1, 10000.0),
        ("transform", "3d", 3, 600.0),
        ("border-radius", "corners differ", 1, 400.0),
    ]
    examples = groups[1].examples()
    assert [e.element for e in examples] == [
        'button "Edge" #1',
        'button "Small" #1',
        'button "Gone" #1',
    ]
    assert examples[0].value == "1 0 0 0 0 1 0 0 0 0 1 0 5 5 0 1"


def test_a_web_card_is_compared_by_its_own_turn_and_its_tapped_face() -> None:
    # The web scales the card element and turns its face, a child region of the same box.
    card = {**element("Beza", [700, 250, 146, 104]), "role": "card"}
    card["style"] = style(transform=[0.8, 0, 0, 0.8, 700, 250])
    face = {**element("", [700, 250, 146, 104]), "role": "region", "parent": 0}
    face["style"] = style(transform=[0, 1, -1, 0, 0, 0])
    drawn = styles.as_drawn([card, face], 0)["style"]
    assert isinstance(drawn, dict)
    assert drawn["transform"] == [0.0, 0.8, -0.8, 0.0, 700, 250]
    # Beryl 3D's tapped card: a quarter turn with no scale.
    assert support.same(drawn["transform"], [0, 1, -1, 0, 710, 280], "transform")
    # A card whose face is not turned is itself.
    face["style"] = style()
    assert styles.as_drawn([card, face], 0) is card


def test_a_card_back_is_compared_either_way_up() -> None:
    web = {**element("", [0, 0, 60, 80]), "role": "card", "name": ""}
    web["style"] = style(transform=[-1, 0.04, -0.04, -1, 700, 40])
    b3d = {**element("", [0, 0, 60, 80]), "role": "card", "name": ""}
    b3d["style"] = style(transform=[1, -0.04, 0.04, 1, 700, -100])
    pairs: list[dict[str, object]] = [{"web": 0, "b3d": 0, "pass": "key"}]
    assert styles.measure([web], [b3d], pairs).differences == []
    # A face shown upside down is a difference.
    web["name"] = b3d["name"] = "Island"
    assert len(styles.measure([web], [b3d], pairs).differences) == 1


def test_a_card_turning_partway_is_found_by_the_middle_of_its_face() -> None:
    # Partway through tapping, the face's box is the larger box of a turned card, but its
    # middle is still the card's.
    card = {**element("Island", [80, 590, 140, 92]), "role": "card"}
    card["style"] = style(transform=[0.9, 0, 0, 0.9, 80, 590])
    face = {**element("", [71, 557, 158, 158]), "role": "region", "parent": 0}
    face["style"] = style(transform=[0.7071, 0.7071, -0.7071, 0.7071, 0, 0])
    drawn = styles.as_drawn([card, face], 0)["style"]
    assert isinstance(drawn, dict)
    assert support.same(
        drawn["transform"], [0.71, 0.71, -0.71, 0.71, 0, 0], "transform"
    )
    # A region whose middle is elsewhere is not the card's face.
    face["box"] = [300, 557, 158, 158]
    assert styles.as_drawn([card, face], 0) is card
