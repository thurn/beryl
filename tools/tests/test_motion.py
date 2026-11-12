import copy
import dataclasses
import json
import random
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import conventions, motion, selection
from beryl_tools.video_measure import Clip

# The values report: Beryl's own values beside Arena's, with no verdict.


def test_the_values_report_shows_beryls_values_beside_arenas_and_holds_none() -> None:
    data, reference = conventions.load(), motion.motion()
    emerge = next(v for v in data["verbs"] if v["name"] == "emerge")
    # A value far from Arena's is Beryl's to choose.
    emerge["values"]["duration"] = 5000
    report = motion.values_report(data, reference)
    [line] = [l for l in report if l.startswith("emerge. ")]
    assert "Beryl: 5000 ms" in line and "Arena: duration 300 ms (240 to 350)" in line
    assert any(
        l.startswith("emerge (triggered ability). Beryl: 275 ms") for l in report
    )
    assert "tap. Exempt: exception 2." in report


def test_a_case_plays_its_own_values_and_its_verbs_where_it_has_none() -> None:
    verb = {
        "values": {"duration": 400, "delay": 100, "stagger": 50, "curve": "ease-out"},
        "cases": {
            "draw": {"duration": 900, "delay": None, "stagger": 0, "curve": None}
        },
    }
    assert motion.values_of(verb, "draw") == {
        "duration": 900,
        "delay": 100,
        "stagger": 0,
        "curve": "ease-out",
    }
    assert motion.values_of(verb, "discard") == verb["values"]
    assert motion.values_of(verb, None) == verb["values"]


def test_a_number_is_held_within_a_tick_or_a_share_of_its_value() -> None:
    reference = motion.motion()
    tick = 1000 / 60
    assert not motion.held(reference, "fly", "duration", 1140, 1000, None, tick).failed
    assert motion.held(reference, "fly", "duration", 1160, 1000, None, tick).failed
    # A short value is held to a tick, as the stage plays a step on whole ticks.
    assert not motion.held(reference, "fly", "duration", 49, 33, None, tick).failed
    assert motion.held(reference, "fly", "duration", 50, 33, None, tick).failed
    # Arena's numbers are shown beside it, as reference.
    line = motion.held(reference, "fly", "duration", 600, 600, "draw", tick).line()
    assert "Beryl 600 ms" in line and "Arena 1084 ms (1050 to 1117)" in line


# The capture: a step held to its verb's values.


def test_each_step_of_the_slice_has_values_of_its_case() -> None:
    cases = {v["name"]: set(v.get("cases", {})) for v in conventions.load()["verbs"]}
    item = {"kind": "ability", "triggered": True}
    steps: list[tuple[dict[str, Any], str]] = [
        ({"verb": "fly", "from": "hand", "object": {"zone": "stack"}}, "yours"),
        ({"verb": "fly", "from": "hand", "object": {"zone": "stack"}}, "theirs"),
        ({"verb": "fly", "from": "stack", "object": {"zone": "battlefield"}}, "yours"),
        ({"verb": "emerge", "id": 1, "item": item}, "yours"),
        (
            {"verb": "appear", "from": "nowhere", "object": {"zone": "battlefield"}},
            "yours",
        ),
        (
            {
                "verb": "vanish",
                "from": "stack",
                "subject": {"kind": "ability", "id": 1},
            },
            "yours",
        ),
    ]
    for act, seat in steps:
        case = motion.case_of(act, seat)
        assert case in cases[str(act["verb"])], (act, seat, case)


def test_a_card_that_leaves_is_held_to_the_case_of_where_it_goes_or_why() -> None:
    def leaves(zone: str, why: str) -> dict[str, Any]:
        subject = {"kind": "object", "slot": 1}
        return {
            "verb": "vanish",
            "subject": subject,
            "why": why,
            "object": {"zone": zone},
        }

    assert motion.case_of(leaves("graveyard", "destroy"), "yours") == "destroyed"
    assert motion.case_of(leaves("graveyard", "counter"), "yours") == "countered"
    assert motion.case_of(leaves("graveyard", "sacrifice"), "yours") == "sacrificed"
    # A spell an effect returns to its hand is bounced, whatever the effect is called.
    assert motion.case_of(leaves("hand", "effect"), "yours") == "bounced"
    assert motion.case_of(leaves("exile", "effect"), "theirs") == "exiled"
    # The opponent's spell is theirs from either seat's capture.
    theirs = {"zone": "stack", "controller": "opponent"}
    cast = {"verb": "fly", "from": "hand", "object": theirs}
    assert motion.case_of(cast, "yours") == "opponent spell cast"
    # A draw is held up only for the player who reads it; an effect's card is no draw.
    hand = {"zone": "hand", "owner": "you", "name": "Island"}
    drawn = {"verb": "fly", "from": "library", "why": "draw", "object": hand}
    assert motion.case_of(drawn, "yours") == "draw"
    unseen = {**drawn, "object": {"zone": "hand", "owner": "opponent", "name": None}}
    assert motion.case_of(unseen, "theirs") == "opponent draw"
    assert motion.case_of({**drawn, "why": "effect"}, "yours") is None


def token_steps(second_from: int, second_to: int) -> dict[str, Any]:
    """The capture's record of two tokens made by one effect, the second played over
    `second_from` to `second_to`."""
    act = {"verb": "appear", "from": "nowhere", "object": {"zone": "battlefield"}}
    steps = [
        {"n": 12, "verb": "appear", "act": act, "from": 2035, "to": 2068},
        {"n": 13, "verb": "appear", "act": act, "from": second_from, "to": second_to},
    ]
    return {"seat": "yours", "tick": 1000 / 60, "steps": steps}


def test_a_step_is_held_to_its_duration_and_a_batch_to_its_stagger() -> None:
    data: dict[str, Any] = conventions.load()
    reference: dict[str, Any] = motion.motion()

    def failed(captured: dict[str, Any]) -> list[tuple[int, str]]:
        verdicts = motion.timing_verdicts(data, reference, captured)
        return [(n, v.measure) for n, vs in enumerate(verdicts) for v in vs if v.failed]

    # Tokens appear 33 ms apart, each in 33 ms.
    assert failed(token_steps(2068, 2101)) == []
    assert failed(token_steps(2135, 2168)) == [(0, "stagger")]
    assert failed(token_steps(2068, 2268)) == [(1, "duration")]
    # Held to Beryl's values, whatever Arena's are.
    token = next(v for v in data["verbs"] if v["name"] == "appear")["cases"]["token"]
    token["duration"], token["stagger"] = 200, 100
    assert failed(token_steps(2135, 2335)) == [(0, "duration")]
    # Steps of the batch that are not next to each other in the script are no batch.
    apart = token_steps(2500, 2700)
    apart["steps"][1]["n"] = 20
    assert failed(apart) == [(0, "duration")]


def test_a_look_at_several_cards_lifts_them_one_after_another() -> None:
    data: dict[str, Any] = conventions.load()
    reference: dict[str, Any] = motion.motion()
    lift = next(v for v in data["verbs"] if v["name"] == "lift")["values"]
    one, apart = lift["duration"], lift["stagger"]

    def failed(count: int, took: float) -> list[str]:
        handling = {"zone": "library", "side": "opponent", "count": count}
        act = {"verb": "lift", "span": "begin", "handling": handling}
        steps = [{"n": 3, "verb": "lift", "act": act, "from": 100, "to": 100 + took}]
        captured = {"seat": "yours", "tick": 1000 / 60, "steps": steps}
        verdicts = motion.timing_verdicts(data, reference, captured)
        return [v.measure for vs in verdicts for v in vs if v.failed]

    assert failed(1, one) == []
    assert failed(2, one + apart) == []
    assert failed(2, one) == ["duration"]


def test_a_steps_travel_and_curve_are_held_to_its_curve_and_its_sizes_only_shown() -> (
    None
):
    data: dict[str, Any] = conventions.load()
    reference: dict[str, Any] = motion.motion()
    resolves = "permanent resolves"

    def failed(verb: str, case: str | None, found: motion.Seen) -> list[str]:
        out = motion.capture_verdicts(data, reference, verb, case, found)
        return [v.measure for v in out if v.failed]

    # Its sizes are Beryl's, shown beside Arena's however far from them.
    big = motion.Seen(travels=True, half=0.65, start=9.0, peak=9.0)
    out = motion.capture_verdicts(data, reference, "fly", resolves, big)
    assert not any(v.failed for v in out)
    [peak] = [v for v in out if v.measure == "peak"]
    assert peak.want is None and peak.arena and "Arena" in peak.line()
    # A flight that does not ease, or a token that flies in, is out.
    even = motion.Seen(travels=True, half=0.5)
    assert failed("fly", "own spell cast", even) == ["curve"]
    assert failed("appear", "token", motion.Seen(travels=True, half=0.6)) == ["travel"]
    # A case with no curve of its own is held to its verb's, and one with its own to that.
    assert failed("fly", "discard", even) == ["curve"]
    assert failed("fly", "land played", motion.Seen(travels=True, half=0.6)) == [
        "curve"
    ]
    for verb in data["verbs"]:
        if verb["name"] == "fly":
            verb["cases"]["land played"]["curve"] = "linear"
    assert failed("fly", "land played", even) == []
    # A verb with no curve is held to nothing it travels.
    assert failed("strike", None, even) == []
    # What the tracker could not follow of a step held to no travel is shown, not failed.
    lunge = motion.Seen(travels=False, failure="2 things moved")
    said = motion.capture_verdicts(data, reference, "strike", "blocked attacker", lunge)
    assert [(v.measure, v.seen, v.failed) for v in said] == [
        ("tracked", "2 things moved", False)
    ]
    lost = motion.Seen(travels=True, failure="2 things moved")
    assert failed("emerge", "triggered ability", lost) == ["tracked"]


def test_a_verb_the_capture_does_not_play_is_one_of_the_suites() -> None:
    suite = selection.verb_suite()["verbs"]
    assert isinstance(suite, list)
    assert set(motion.MORE_ONLY) <= {str(v) for v in suite}


def test_a_step_not_screenshotted_is_held_to_its_timing_and_says_why(
    tmp_path: Path,
) -> None:
    data: dict[str, Any] = conventions.load()
    reference: dict[str, Any] = motion.motion()
    look = {"verb": "lift", "span": "begin"}
    play = tmp_path / "desktop" / "theirs.scry"
    play.mkdir(parents=True)
    unshot = "its subject moves again just after it"
    steps = [
        {"n": 10, "verb": "lift", "act": look, "from": 2116, "to": 2549},
        {"n": 12, "verb": "lift", "act": look, "from": 2574, "to": 2700},
    ]
    steps[0]["unshot"] = unshot
    for step in steps:
        step["folder"] = f"{step['n']}-lift"
    captured = {"seat": "theirs", "layout": "desktop", "tick": 1000 / 60}
    (play / "steps.json").write_text(json.dumps({**captured, "steps": steps}))
    measured = motion.measure_capture(data, reference, tmp_path)
    said = {name: [(v.measure, v.failed) for v in vs] for _, name, vs in measured}
    # Named by its play's folder, held to its duration and no frames read; why it was not
    # screenshotted is shown, held to nothing.
    assert said == {
        "desktop/theirs.scry/10-lift": [("duration", False), ("frames", False)],
        "desktop/theirs.scry/12-lift": [("duration", True)],
    }


WIDTH, HEIGHT, FPS = 240, 135, 60
CARD_W, CARD_H = 30, 42


def flight(
    ease: Callable[[float], float],
    scale_from: float,
    length: float = 0.75,
    travel: tuple[float, float] = (160, -60),
    copy: bool = True,
    plate: bool = False,
) -> Clip:
    """Frames of one card crossing a noisy table by `travel` pixels in 0.45 s from 0.1 s, eased
    by `ease` and growing (or shrinking) from `scale_from` of its size to it; with `plate`,
    the table bare beside them. A growing one is a `copy`, as an emerging ability is: it is not
    there before it leaves."""
    rng = random.Random(3)
    table = [rng.randint(30, 100) for _ in range(WIDTH * HEIGHT)]
    times = [n / FPS for n in range(round(length * FPS) + 1)]
    frames: list[bytes] = []
    for t in times:
        p = ease(min(1.0, max(0.0, (t - 0.1) / 0.45)))
        pixels = list(table)
        if copy and scale_from < 1 and t < 0.1:
            frames.append(bytes(pixels))
            continue
        scale = scale_from + (1 - scale_from) * p
        w, h = round(CARD_W * scale), round(CARD_H * scale)
        cx, cy = 40 + travel[0] * p, 100 + travel[1] * p
        left, top = round(cx - w / 2), round(cy - h / 2)
        for y in range(max(0, top), min(HEIGHT, top + h)):
            for x in range(max(0, left), min(WIDTH, left + w)):
                pixels[y * WIDTH + x] = 200
        frames.append(bytes(pixels))
    bare = bytes(table) if plate else None
    return Clip(WIDTH, HEIGHT, times, frames, (WIDTH, HEIGHT), plate=bare)


def bezier(x1: float, y1: float, x2: float, y2: float) -> Callable[[float], float]:
    """CSS's cubic-bezier(x1, y1, x2, y2) as progress over time."""

    def at(p: float) -> float:
        low, high = 0.0, 1.0
        for _ in range(40):
            m = (low + high) / 2
            x = 3 * (1 - m) ** 2 * m * x1 + 3 * (1 - m) * m * m * x2 + m**3
            low, high = (m, high) if x < p else (low, m)
        m = (low + high) / 2
        return 3 * (1 - m) ** 2 * m * y1 + 3 * (1 - m) * m * m * y2 + m**3

    return at


@pytest.mark.parametrize(
    "ease, scale_from, curve",
    [
        (lambda p: p, 1.0, "even"),
        (lambda p: p, 0.3, "even"),
        (bezier(0, 0, 0.58, 1), 1.0, "ease-out"),
        (bezier(0, 0, 0.58, 1), 3.0, "ease-out"),
        (bezier(0.42, 0, 1, 1), 1.0, "ease-in"),
        (bezier(0.42, 0, 1, 1), 0.3, "ease-in"),
    ],
)
def test_the_tracker_tells_how_a_flight_eases_whether_it_grows_or_shrinks(
    ease: Callable[[float], float], scale_from: float, curve: str
) -> None:
    found = motion.seen(flight(ease, scale_from))
    assert found.travels and found.failure is None
    assert found.curve == curve, found.half
    assert found.start == pytest.approx(scale_from, rel=0.25)


def test_over_a_plate_a_card_is_followed_where_its_path_overlaps_its_ends() -> None:
    """A card lifted off a pile, as an opponent's library card is (by-w8gv.21.46): growing
    to its size from 0.7 of it, it rises less than its height, so it covers where it lay and
    where it rests most of the way and the first and last frames leave only slivers of it.
    Against a plate it is whole in every frame."""

    def lift(plate: bool) -> Clip:
        return flight(
            bezier(0, 0, 0.58, 1), 0.7, travel=(6, -36), copy=False, plate=plate
        )

    assert motion.seen(lift(False)).curve is None
    found = motion.seen(lift(True))
    assert found.travels and found.failure is None
    assert found.curve == "ease-out", found.half
    assert found.start == pytest.approx(0.7, rel=0.1)


def test_over_a_plate_a_card_changing_where_it_landed_has_stopped() -> None:
    """An effect that pulses on a card once it lands, to the clip's end, blends its edge with
    the table: its box shifts a pixel and back, and every one of its pixels changes, without
    it moving."""
    clip = flight(bezier(0, 0, 0.58, 1), 1.0, plate=True)
    assert clip.plate is not None
    landed = round(0.6 * FPS)
    frames = list(clip.frames[:landed])
    for n, frame in enumerate(clip.frames[landed:]):
        lit = bytes(215 if v == 200 else v for v in frame)
        if n % 2:
            top = lit.index(215) // WIDTH
            row = slice(top * WIDTH, (top + 1) * WIDTH)
            lit = lit[: row.start] + clip.plate[row] + lit[row.stop :]
        frames.append(lit)
    found = motion.seen(dataclasses.replace(clip, frames=frames))
    assert found.travels and found.failure is None
    assert found.curve == "ease-out", found.half


def test_a_card_that_comes_to_be_in_place_does_not_travel() -> None:
    landed = flight(lambda p: p, 1.0)
    absent = bytes(landed.frames[0][:0]) + bytes(
        random.Random(3).randint(30, 100) for _ in range(WIDTH * HEIGHT)
    )
    frames = [absent] * 3 + [landed.frames[-1]] * 3
    clip = Clip(WIDTH, HEIGHT, landed.times[:6], frames, (WIDTH, HEIGHT))
    assert motion.seen(clip) == motion.Seen(travels=False)


def test_a_change_in_place_is_held_to_its_case() -> None:
    """A turn by which way it goes, a strike by what it lands on and a counter by its kind, as
    the stage times them (ui/src/stage/timing.ts caseOf); a life total alone has no case.
    """
    tapped = {"verb": "tap", "object": {"tapped": True}}
    assert motion.case_of(tapped, "yours") == "tap"
    assert (
        motion.case_of({"verb": "tap", "object": {"tapped": False}}, "yours") == "untap"
    )
    at_player = {"verb": "strike", "to": {"kind": "player", "player": "opponent"}}
    assert motion.case_of(at_player, "yours") == "unblocked attacker"
    at_blocker = {"verb": "strike", "to": {"kind": "object", "slot": 3}}
    assert motion.case_of(at_blocker, "theirs") == "blocked attacker"

    def counted(tally: dict[str, object]) -> str | None:
        return motion.case_of({"verb": "count", "tally": tally}, "yours")

    assert counted({"kind": "counters", "counter": "loyalty"}) == "loyalty"
    assert counted({"kind": "counters", "counter": "plusOne"}) == "counter"
    assert counted({"kind": "life", "player": "you", "life": 17}) is None


def test_a_step_that_moves_no_card_is_held_to_its_case() -> None:
    def case(act: dict[str, object]) -> str | None:
        return motion.case_of(act, "yours")

    assert case({"verb": "streak", "stream": {"kind": "mana"}}) == "mana"
    assert case({"verb": "streak", "stream": {"kind": "damage"}}) == "damage"
    assert case({"verb": "announce", "notice": {"kind": "turn"}}) == "turn banner"
    assert case({"verb": "announce", "notice": {"kind": "scry"}}) == "scry pill"
    assert case({"verb": "announce", "notice": {"kind": "coin"}}) == "coin"
    assert case({"verb": "dwell", "subject": {"kind": "player"}}) == "game's end"
    assert case({"verb": "dwell", "subject": {"kind": "object"}}) is None
    assert case({"verb": "set", "fact": {"kind": "turn"}}) == "step begins"
    assert case({"verb": "set", "fact": {"kind": "library"}}) == "library shuffled"
    assert case({"verb": "set", "fact": {"kind": "object"}}) is None
    # Each is a case its verb's values have.
    verbs = {v["name"]: v for v in conventions.load()["verbs"]}
    for verb, named in (
        ("streak", ("mana", "damage")),
        ("announce", ("turn banner", "scry pill", "coin")),
        ("dwell", ("game's end",)),
        ("set", ("step begins", "library shuffled")),
    ):
        assert set(named) <= set(verbs[verb]["cases"])


def test_steps_alone_in_their_beats_are_held_to_no_stagger() -> None:
    """Two steps of the turn begin one after the other, each a beat of its own: the second
    is a whole step after the first, which is no stagger of a batch."""
    act = {"verb": "set", "fact": {"kind": "turn"}}
    steps = [{"n": n, "verb": "set", "act": act, "case": "step begins"} for n in (0, 1)]
    captured = {
        "seat": "yours",
        "tick": 16,
        "steps": [
            {**steps[0], "from": 0, "to": 58},
            {**steps[1], "from": 58, "to": 116},
        ],
    }
    verdicts = motion.timing_verdicts(conventions.load(), motion.motion(), captured)
    assert [len(found) for found in verdicts] == [1, 1]
    assert not any(v.failed for found in verdicts for v in found)


def test_an_exempt_verb_is_listed() -> None:
    data = {"verbs": [{"name": "tap", "exempt": 2}, {"name": "fly"}]}
    assert motion.exempted(data) == {"tap": 2}


def test_a_step_is_timed_by_the_case_the_page_told(tmp_path: Path) -> None:
    """A life total's case is the page's, which reads it from the table the step found; a tap,
    exempt and never screenshotted, is still held to its own duration."""
    data: dict[str, Any] = copy.deepcopy(conventions.load())
    count = next(verb for verb in data["verbs"] if verb["name"] == "count")
    count["cases"]["life gain"]["duration"] = 500
    count["cases"]["life loss"]["duration"] = 300
    turn = next(verb for verb in data["verbs"] if verb["name"] == "tap")
    turn["cases"]["untap"]["duration"] = 100
    reference: dict[str, Any] = motion.motion()
    play = tmp_path / "desktop" / "yours.combat"
    play.mkdir(parents=True)
    life = {"verb": "count", "tally": {"kind": "life", "player": "you", "life": 23}}
    tap = {"verb": "tap", "object": {"slot": 4, "tapped": False}}
    steps: list[dict[str, Any]] = [
        {"n": 3, "verb": "count", "act": life, "case": "life gain", "to": 500},
        {"n": 5, "verb": "count", "act": life, "case": "life loss", "to": 500},
        {"n": 7, "verb": "tap", "act": tap, "case": "untap", "to": 100},
    ]
    for step in steps:
        step |= {"from": 0, "folder": f"{step['n']}-{step['verb']}", "unshot": "-"}
    captured = {"seat": "yours", "layout": "desktop", "tick": 1000 / 60}
    (play / "steps.json").write_text(json.dumps({**captured, "steps": steps}))
    measured = motion.measure_capture(data, reference, tmp_path)
    said = {
        name: [(v.measure, v.case, v.failed) for v in vs if v.measure == "duration"]
        for _, name, vs in measured
    }
    assert said == {
        "desktop/yours.combat/3-count": [("duration", "life gain", False)],
        "desktop/yours.combat/5-count": [("duration", "life loss", True)],
        "desktop/yours.combat/7-tap": [("duration", "untap", False)],
    }


def test_a_stagger_after_a_step_held_back_is_what_the_stage_waits() -> None:
    """The stage begins a beat's steps in order: a strike held back by the delay of a life
    total's count before it begins after its turn, and the like strike after it then begins
    less than a stagger later, by when its own turn comes."""
    data: dict[str, Any] = copy.deepcopy(conventions.load())
    strike = next(verb for verb in data["verbs"] if verb["name"] == "strike")
    strike["cases"]["blocked attacker"]["stagger"] = 220
    reference: dict[str, Any] = motion.motion()
    at_blocker: dict[str, Any] = {"verb": "strike", "to": {"kind": "object", "slot": 3}}
    case: str = "blocked attacker"
    held_back = {"n": 20, "turn": 3288, "from": 3388, "to": 4438}
    after = {"n": 21, "turn": 3508, "from": 3508, "to": 4558}

    def stagger(*steps: dict[str, Any]) -> list[tuple[str, bool]]:
        played = [
            {**s, "verb": "strike", "act": at_blocker, "case": case} for s in steps
        ]
        captured = {"seat": "yours", "tick": 1000 / 60, "steps": played}
        verdicts = motion.timing_verdicts(data, reference, captured)[0]
        return [(v.want or "", v.failed) for v in verdicts if v.measure == "stagger"]

    assert stagger(held_back, after) == [("120 ms (±18)", False)]
    on_turn = {**held_back, "turn": 3388}
    assert stagger(on_turn, after) == [("220 ms (±33)", True)]


def test_a_fights_two_blows_are_held_to_one_time() -> None:
    """Two creatures that strike each other do so as one: the blow dealt back begins with the
    strike it answers, and one stagger later would fail."""
    data: dict[str, Any] = copy.deepcopy(conventions.load())
    strike = next(verb for verb in data["verbs"] if verb["name"] == "strike")
    strike["cases"]["blocked attacker"]["stagger"] = 220
    reference: dict[str, Any] = motion.motion()

    def blow(source: int, slot: int) -> dict[str, Any]:
        return {
            "verb": "strike",
            "source": source,
            "to": {"kind": "object", "slot": slot},
        }

    def stagger(later: int) -> list[bool]:
        steps = [
            {"n": 20, "turn": 0, "from": 0, "to": 1050, "act": blow(1, 9)},
            {"n": 21, "turn": 0, "from": later, "to": later + 1050, "act": blow(9, 1)},
        ]
        played = [
            {**step, "verb": "strike", "case": "blocked attacker"} for step in steps
        ]
        captured = {"seat": "yours", "tick": 1000 / 60, "steps": played}
        verdicts = motion.timing_verdicts(data, reference, captured)[0]
        return [v.failed for v in verdicts if v.measure == "stagger"]

    assert stagger(0) == [False]
    assert stagger(220) == [True]
