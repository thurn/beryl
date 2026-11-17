"""`just b3d match motion`: pairing elements over a span of frames, following a Beryl 3D card
whose locator changes when it is revealed, measuring how far two tracks lie apart, handing the
web page its views on Beryl 3D's schedule, and the plot of a pair."""

from __future__ import annotations

from beryl_tools.beryl3d.match import motion, plot, tracks


def card(
    box: list[int], *, key: str | None = None, track: int | None = None
) -> dict[str, object]:
    element: dict[str, object] = {
        "key": key,
        "role": "card",
        "name": "",
        "object": None,
        "box": box,
        "z": 0,
        "parent": None,
        "style": {"opacity": 1.0},
        "text": [],
    }
    if track is not None:
        element["track"] = track
    return element


def test_a_revealed_card_keeps_its_track_and_is_reported_by_its_latest_locator() -> (
    None
):
    frames = [
        [card([0, 0, 10, 10], key="card #259")],
        [card([5, 0, 10, 10], key='card "Snapcaster Mage" #259')],
    ]
    latest = tracks.name_tracks(frames)
    assert latest == {"#259": 'card "Snapcaster Mage" #259'}
    assert list(tracks.tracks(frames)) == ["#259"]


def test_each_element_keeps_the_partner_it_was_paired_with_in_most_frames() -> None:
    # Two cards of one role cross between frames 1 and 2. The matcher pairs each frame by the
    # nearest box, which swaps them in the frame where they meet; over the span each keeps the
    # web card that follows it.
    a = [[0, 0, 10, 10], [8, 0, 10, 10], [12, 0, 10, 10], [20, 0, 10, 10]]
    b = [[20, 0, 10, 10], [13, 0, 10, 10], [9, 0, 10, 10], [0, 0, 10, 10]]
    web = [[card(a[i], track=1), card(b[i], track=2)] for i in range(4)]
    b3d = [[card(a[i], key="card #1"), card(b[i], key="card #2")] for i in range(4)]
    b3d[2] = [card(b[2], key="card #1"), card(a[2], key="card #2")]
    tracks.name_tracks(b3d)
    assert tracks.partners(web, b3d) == {"#1": 1, "#2": 2}


def test_a_difference_names_the_frame_of_the_worst_position_size_and_opacity() -> None:
    web: tracks.Track = [
        tracks.Sample(0, 0, 10, 10, 1.0),
        tracks.Sample(10, 0, 10, 10, 1.0),
        tracks.Sample(20, 0, 10, 10, 0.5),
    ]
    b3d: tracks.Track = [
        None,
        tracks.Sample(16, 8, 10, 10, 1.0),
        tracks.Sample(20, 0, 14, 10, 0.2),
    ]
    d = tracks.difference(web, b3d)
    assert (d.position, d.size, d.shared) == (
        tracks.Worst(10.0, 1),
        tracks.Worst(4.0, 2),
        2,
    )
    assert d.opacity is not None and d.opacity.frame == 2
    assert abs(d.opacity.amount - 0.3) < 1e-9
    assert tracks.present(b3d) == "1-2"
    assert tracks.present([web[0], None, None, web[1], None]) == "0, 3"


def test_an_element_moves_when_its_box_or_opacity_changes_or_it_comes_or_goes() -> None:
    still = tracks.Sample(0, 0, 10, 10, 1.0)
    assert not tracks.moves([still, tracks.Sample(0.3, 0, 10, 10, 1.0)])
    assert tracks.moves([still, tracks.Sample(0, 0, 11, 10, 1.0)])
    assert tracks.moves([still, tracks.Sample(0, 0, 10, 10, 0.9)])
    assert tracks.moves([still, None])


def update(status: str) -> dict[str, object]:
    return {
        "kind": "event",
        "event": "game-update",
        "payload": {"view": {"status": {"kind": status}}},
    }


def test_the_web_page_is_handed_each_batch_of_views_when_beryl_3d_was() -> None:
    # Five views: the first two arrive at 0 ms, the next two at 400 ms and the last at 900 ms.
    # The third is still playing, so one `advance` hands over the fourth with it.
    messages = [
        update("waiting"),
        update("waiting"),
        update("playing"),
        update("waiting"),
        update("waiting"),
    ]
    given = [0.0, 0.0, 400.0, 400.0, 900.0]
    opened, advances, notes = motion.schedule(messages, given, 400.0, 1000.0)
    assert (opened, advances, notes) == (2, [[0.0, 1], [500.0, 1]], [])
    # A span whose batch holds only the playing view gets the next view early, and says so.
    _, _, early = motion.schedule(messages, [0.0, 0.0, 400.0, 600.0], 400.0, 500.0)
    assert len(early) == 1 and "1 more events" in early[0]


def test_a_plot_draws_both_sides_breaking_a_line_where_its_element_is_off_screen() -> (
    None
):
    web: tracks.Track = [tracks.Sample(i * 10, 0, 10, 10, 1.0) for i in range(4)]
    b3d: tracks.Track = [
        tracks.Sample(0, 0, 10, 10, 1.0),
        tracks.Sample(10, 0, 10, 10, 1.0),
        None,
        tracks.Sample(30, 0, 12, 10, 1.0),
    ]
    picture = plot.svg('card "Island" #2', [0, 16.7, 33.3, 50], web, b3d)
    assert picture.startswith("<svg") and "card &quot;Island&quot; #2" in picture
    # Four panels: the web's line whole in each; Beryl 3D's in two pieces, the last a dot.
    assert picture.count(f'stroke="{plot.WEB}"') == 4
    assert picture.count(f'stroke="{plot.B3D}"') == 4
    assert picture.count(f'r="2" fill="{plot.B3D}"') == 4
