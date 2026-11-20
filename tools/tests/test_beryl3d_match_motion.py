"""`just b3d match motion`: pairing elements over a span of frames, following a Beryl 3D card
whose locator changes when it is revealed, measuring how far two tracks lie apart, handing the
web page its views on Beryl 3D's schedule, and the plot of a pair."""

from __future__ import annotations

from pathlib import Path

from beryl_tools.beryl3d.match import clock, motion, plot, tracks


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
    opened, advances, notes = clock.schedule(messages, given, 400.0, 1000.0)
    assert (opened, advances, notes) == (2, [[0.0, 1], [500.0, 1]], [])
    # A span whose batch holds only the playing view gets the next view early, and says so.
    _, _, early = clock.schedule(messages, [0.0, 0.0, 400.0, 600.0], 400.0, 500.0)
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


def test_motion_all_ranks_items_by_summed_weighed_deviation_worst_first() -> None:
    from beryl_tools.beryl3d.match import motion_census

    times = [0.0, 20.0, 40.0, 60.0]
    web: tracks.Track = [tracks.Sample(i * 10, 0, 10, 10, 1.0) for i in range(4)]
    # Beryl 3D's card starts a frame late and ends where the web's does: its center is 10, 10
    # and 0 points off in the last three frames, and its motion starts 20 ms later.
    late: tracks.Track = [web[0], web[0], web[1], web[3]]
    d = tracks.deviation(web, late, times)
    assert (d.position, d.size, d.opacity) == (5.0, 0.0, 0.0)
    assert d.timing == 20.0
    assert d.most() == "position" and d.total() == 7.0
    # A fade that is half as opaque throughout differs most in opacity.
    faded = [tracks.Sample(s.x, s.y, s.width, s.height, 0.5) for s in web if s]
    assert tracks.deviation(web, list(faded), times).most() == "opacity"
    # An element only one side moves counts how far it goes.
    assert tracks.alone(web).position == 30.0

    def span(name: str, end: str) -> motion_census.Span:
        return motion_census.Span("verbs", name, "verbs-zones", "you", "0ms", end)

    def row(key: str, d: tracks.Deviation, effect: str = "") -> dict[str, object]:
        return {"key": key, "effect": effect, **vars(d)}

    fly, tap, gone = span("fly", "50ms"), span("tap", "60ms"), span("lift", "70ms")
    # Effects that begin with the fly step share its frames, and so its result, but each is
    # scored from its own elements only.
    glow = motion_census.Span("effects", "glow", "verbs-zones", "you", "0ms", "50ms")
    flash = motion_census.Span("effects", "flash", "verbs-zones", "you", "0ms", "50ms")
    still = tracks.Deviation(0.0, 0.0, 0.0, 0.0)
    results: dict[motion_census.Where, dict[str, object]] = {
        tap.where(): {"rows": [row("card #1", d), row("card #2", d)], "notes": []},
        fly.where(): {
            "rows": [
                row("card #3", tracks.Deviation(1, 0, 0.2, 0)),
                row("card #4", still),
                row('effect "glow" on 3', tracks.Deviation(0, 0, 0.15, 0), "glow"),
            ],
            "notes": [],
        },
        gone.where(): {"failed": "b3d tree failed"},
    }
    ranked = motion_census.rank([gone, tap, fly, glow, flash], results)
    assert [r.span.name for r in ranked] == ["fly", "glow", "tap", "flash", "lift"]
    assert (ranked[0].element, ranked[0].most, ranked[0].total) == (
        "card #3",
        "opacity",
        36.0,
    )
    assert ranked[1].total == 15.0 and ranked[1].element == 'effect "glow" on 3'
    assert ranked[2].total == 14.0
    assert (ranked[3].total, ranked[3].element) == (
        0.0,
        "neither side draws it in its span",
    )
    assert ranked[4].failed == "b3d tree failed"
    lines = motion_census.ranking_lines(ranked)
    assert lines[0].startswith("  1. verb fly  36.0  card #3  most: opacity")
    assert (
        lines[1]
        .strip()
        .startswith("just b3d match motion verbs-zones --from 0ms --to 50ms")
    )
    assert lines[2].startswith("  2. effect glow  15.0")
    assert lines[-1] == "  5. verb lift: not measured: b3d tree failed"


def test_a_web_element_inside_a_partnered_one_is_not_reported_alone() -> None:
    # The web card (track 1) holds its text panel (track 2), which moves with it; Beryl 3D has
    # the card alone. A second web element, outside the card, moves on its own.
    def web_frame(x: int) -> list[dict[str, object]]:
        panel = card([x, 0, 10, 4], track=2) | {"role": "region", "parent": 0}
        other = card([x, 50, 10, 10], track=3) | {"role": "region"}
        return [card([x, 0, 10, 10], track=1), panel, other]

    web = [web_frame(0), web_frame(10)]
    b3d = [
        [card([0, 0, 10, 10], key="card #1")],
        [card([10, 0, 10, 10], key="card #1")],
    ]
    latest = tracks.name_tracks(b3d)
    pairs, lonely_b3d, lonely_web = motion.compared(web, b3d, latest, None)
    assert [p.key for p in pairs] == ["card #1"] and lonely_b3d == []
    assert [lonely.key for lonely in lonely_web] == ['region "" (frames 0-1)']


def test_a_web_element_alone_is_measured_only_while_it_is_on_the_screen() -> None:
    # During a mulligan the web's stage card waits below the window (track 2) and then shows
    # in the library pile: a player sees it come, not move, so it adds nothing to the
    # census's sum. A card that moves on the screen (track 3) adds how far it moves.
    def web_frame(y: int) -> list[dict[str, object]]:
        screen = card([0, 0, 100, 100], track=1) | {"role": "region"}
        waiting = card([10, y, 10, 10], track=2) | {"parent": 0}
        moving = card([y // 4, 10, 10, 10], track=3) | {"parent": 0}
        return [screen, waiting, moving]

    web = [web_frame(200), web_frame(200), web_frame(80)]
    b3d: list[list[dict[str, object]]] = [[], [], []]
    pairs, lonely_b3d, lonely_web = motion.compared(web, b3d, {}, None)
    assert pairs == [] and lonely_b3d == []
    found = {lonely.key: tracks.alone(lonely.track).position for lonely in lonely_web}
    assert found == {'card "" (frames 2)': 0.0, 'card "" (frames 0-2)': 30.0}


def effect(
    box: list[float],
    opacity: float,
    *,
    key: str | None = None,
    track: int | None = None,
) -> dict[str, object]:
    """A `glow` on the card in slot 7, as either side records it."""
    return card([int(n) for n in box], key=key, track=track) | {
        "role": "effect",
        "name": "glow",
        "object": 7,
        "box": box,
        "style": {"opacity": opacity},
    }


def test_an_effect_beryl_3d_stops_drawing_raises_its_own_row_and_no_other() -> None:
    from beryl_tools.beryl3d.match import motion_census

    # The web's glow is a node inside the card that fades over three frames; the card stands
    # still. Beryl 3D draws the same glow, or none.
    fades: list[float] = [1.0, 0.6, 0.2]
    web: list[list[dict[str, object]]] = [
        [
            card([10, 10, 10, 10], track=1) | {"object": 7},
            effect([8, 8, 14, 14], o, track=2) | {"parent": 0},
        ]
        for o in fades
    ]

    def measured(drawn: bool) -> dict[str, object]:
        b3d = [
            [card([10, 10, 10, 10], key="card #7") | {"object": 7}]
            + ([effect([8, 8, 14, 14], o, key='effect "glow" on 7')] if drawn else [])
            for o in fades
        ]
        latest = tracks.name_tracks(b3d)
        pairs, lonely_b3d, lonely_web = motion.compared(web, b3d, latest, None)
        found = motion.Measured(
            1, 0.0, [0.0, 16.7, 33.3], [], Path("."), pairs, lonely_b3d, lonely_web
        )
        return motion_census.scored(found, (1600, 900))

    glow = motion_census.Span("effects", "glow", "verbs-zones", "you", "0ms", "50ms")
    drawn, missing = measured(True), measured(False)
    [with_glow] = motion_census.rank([glow], {glow.where(): drawn})
    [without] = motion_census.rank([glow], {glow.where(): missing})
    assert with_glow.total == 0.0
    # Drawn on the web only, the glow adds the whole of its fade: 0.8 of opacity is 80 points.
    assert round(without.total, 6) == 80.0 and "web only" in without.element
    # Where it is strongest is its first frame, at the web node's box.
    assert drawn["peaks"] == {"glow": {"t_ms": 0.0, "box": [8, 8, 14, 14]}}


def test_the_web_page_opens_with_the_views_beryl_3d_had_played_out() -> None:
    # verbs-zones from your seat: views 1 to 3 are given at 183 ms and views 4 to 6 at 458 ms,
    # where view 4's script plays until 1198 ms, view 5's until 1598 ms and view 6's until 2000.
    given = [183.0, 183.0, 183.0, 458.0, 458.0, 458.0]
    ends = clock.ends_of(
        {
            "moments": [
                {"moment": "step:2.0.0", "from_ms": 0, "to_ms": 58},
                {"moment": "step:4.1.0", "from_ms": 828, "to_ms": 1198},
                {"moment": "step:4.0.0", "from_ms": 458, "to_ms": 828},
                {"moment": "step:5.0.0", "from_ms": 1198, "to_ms": 1598},
                {"moment": "step:6.0.0", "from_ms": 1598, "to_ms": 2000},
                {"moment": "view:4", "t_ms": 2000},
            ]
        }
    )
    assert ends == {2: 58.0, 4: 1198.0, 5: 1598.0, 6: 2000.0}
    # At 1000 ms Beryl 3D plays view 4: the page opens with three views and gets the rest live.
    assert clock.first_live(given, ends, 1000.0) == 3
    # At 1300 ms view 4 has ended, but view 5 waited for it, so view 4 is still played live.
    assert clock.first_live(given, ends, 1300.0) == 3
    # Once every script has ended, the page opens with every view.
    assert clock.first_live(given, ends, 2500.0) == 6
    messages = [update("waiting") for _ in given]
    base, opened, advances, notes = clock.handover(
        messages, given, ends, 1000.0, 1100.0, 458.0
    )
    assert (base, opened, advances) == (458.0, 3, [[0.0, 3]])
    assert notes[0].startswith("note: views 4 to 6 were still to play at 1000ms")


def test_the_web_page_runs_its_timers_for_as_long_as_beryl_3d_had_its_latest_view() -> (
    None
):
    # At 2255 ms of `opening-bottom` the page opens at 2165 ms with the views Beryl 3D was given
    # at 0 ms, so its coin, which spins for 1500 ms from when the opening first shows, must have
    # run for 2165 ms already. A page that opens with no view runs no timers first.
    assert clock.preroll([0.0, 0.0, 0.0, 2165.0, 2165.0], 2165.0) == 2165.0
    assert clock.preroll([0.0, 500.0, 2165.0], 2165.0) == 1665.0
    assert clock.preroll([2165.0], 2165.0) == 0.0
