"""`just b3d compare`: which web route shows the moment Beryl 3D draws, and which recording,
seat and window a command line names."""

from __future__ import annotations

import argparse

from beryl_tools.beryl3d import compare


def namespace(recording: str, **given: str | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    compare.arguments(parser)
    args = parser.parse_args([recording, "--at", "end"])
    for key, value in given.items():
        setattr(args, key, value)
    return args


def update() -> dict[str, object]:
    return {"kind": "event", "event": "game-update"}


def test_the_route_releases_the_events_up_to_the_views_game_update() -> None:
    messages: list[dict[str, object]] = [
        {"kind": "response", "command": "start_game"},
        update(),
        {"kind": "event", "event": "notice"},
        update(),
        update(),
    ]
    # Responses are not events; the notice between the first two updates is one.
    assert compare.web_at(messages, 1) == 1
    assert compare.web_at(messages, 2) == 3
    assert compare.web_at(messages, 3) == 4


def test_a_tests_name_gives_its_seat_and_window_unless_the_options_do() -> None:
    assert compare.target(namespace("b3d:screen/start/opponent/1440x900")) == (
        "screen/start",
        "opponent",
        "1440x900",
    )
    assert compare.target(
        namespace("b3d:pay-default/opponent/1440x900", seat="you")
    ) == ("pay-default", "you", "1440x900")


def test_a_recording_alone_is_seen_from_the_viewers_seat_in_its_layouts_window() -> (
    None
):
    assert compare.target(namespace("opening-play")) == (
        "opening-play",
        "you",
        "1600x900",
    )
    assert compare.target(namespace("opening-play", layout="phone")) == (
        "opening-play",
        "you",
        "393x759",
    )


def test_retina_test_names_keep_their_recording_and_scale() -> None:
    args = namespace("b3d:stress-1024/you/1600x900@2")
    assert compare.target(args) == ("stress-1024", "you", "1600x900")
    assert compare.picture_scale(args) == "2"
    assert compare.picture_scale(namespace(args.recording, scale="1")) == "1"
    assert compare.picture_scale(namespace("opening-play")) == "1"
