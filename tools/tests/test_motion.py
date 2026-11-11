import random
from collections.abc import Callable
from typing import Any

import pytest

from beryl_tools import conventions, motion
from beryl_tools.video_measure import Clip

# The values check, on the tree: every verb's duration, delay and stagger within Arena's.


def test_every_verb_holds_to_arenas_numbers() -> None:
    data, reference = conventions.load(), motion.motion()
    assert motion.data_problems(reference, [v["name"] for v in data["verbs"]]) == []
    verdicts, exempt = motion.values_check(data, reference)
    assert [v.line() for v in verdicts if v.failed] == []
    assert {v.verb for v in verdicts} >= {"fly", "emerge", "appear", "vanish"}
    # A verb a numbered exception covers is skipped, and listed.
    assert "tap (exception 2)" in exempt
    assert all(v.verb != "tap" for v in verdicts)


@pytest.mark.parametrize("measure", motion.TIMED)
def test_a_value_moved_out_of_tolerance_fails(measure: str) -> None:
    data, reference = conventions.load(), motion.motion()
    verb = next(v for v in data["verbs"] if v["name"] == "emerge")
    low, high = motion.bounds(
        reference, motion.entries(reference)["emerge"], measure
    ) or (0, 0)
    verb["values"][measure] = round(high) + 1
    failed = [v for v in motion.values_check(data, reference)[0] if v.failed]
    assert [(v.verb, v.measure) for v in failed] == [("emerge", measure)]
    verb["values"][measure] = round(high)
    assert not any(v.failed for v in motion.values_check(data, reference)[0])


def test_a_verb_widens_its_tolerance_or_waives_a_measure_only_with_a_reason() -> None:
    data, reference = conventions.load(), motion.motion()
    names = [v["name"] for v in data["verbs"]]
    verb = next(v for v in data["verbs"] if v["name"] == "emerge")
    verb["values"]["duration"] = 500
    entry = motion.entries(reference)["emerge"]
    entry["tolerance"] = {"share": 1.0}
    assert motion.data_problems(reference, names) != []
    entry["tolerance"]["reason"] = "a test"
    assert not any(v.failed for v in motion.values_check(data, reference)[0])
    del entry["tolerance"]
    entry["waivers"] = [
        {"measure": "duration", "bead": "by-w8gv.21.19", "reason": "a test"}
    ]
    assert motion.data_problems(reference, names) == []
    [waived] = [v for v in motion.values_check(data, reference)[0] if v.fault]
    assert waived.waived == "by-w8gv.21.19" and not waived.failed
    entry["waivers"] = [{"measure": "duration", "reason": "no bead"}]
    assert motion.data_problems(reference, names) != []


def reference_of(numbers: list[int | None], recordings: list[str]) -> dict[str, Any]:
    instances = [
        {"recording": r, "case": "one", "duration_ms": n, "peak": None}
        for n, r in zip(numbers, recordings)
    ]
    summary = conventions.summarize(instances)
    return {
        "tolerance": {"frames": 1, "share": 0.15, "fps": 30, "fps_of": {"sixty": 60}},
        "verbs": [{"verb": "v", "instances": instances, "summary": summary}],
    }


def test_the_tolerance_is_a_frame_of_the_coarsest_recording_or_a_share_of_the_median() -> (
    None
):
    # A share of a long median is more than a frame.
    long = reference_of([1000, 1100, 1200], ["a", "b", "c"])
    assert motion.bounds(long, long["verbs"][0], "duration") == (835.0, 1365.0)
    # A frame at 30 fps is more than a share of a short one, and 60 fps recordings alone
    # give a frame of their own.
    short = reference_of([40, 50, 60], ["a", "sixty", "sixty"])
    low, high = motion.bounds(short, short["verbs"][0], "duration") or (0, 0)
    assert (low, high) == pytest.approx((40 - 1000 / 30, 60 + 1000 / 30))
    sixty = reference_of([40, 50, 60], ["sixty"] * 3)
    low, high = motion.bounds(sixty, sixty["verbs"][0], "duration") or (0, 0)
    assert (low, high) == pytest.approx((40 - 1000 / 60, 60 + 1000 / 60))
    assert motion.bounds(sixty, sixty["verbs"][0], "delay") is None


# The capture: what the tracker finds of a step, held to Arena's case of it.


def test_each_step_of_the_slice_is_held_to_a_case_arena_measured() -> None:
    cases = {
        verb: set(entry["summary"].get("cases", {}))
        for verb, entry in motion.entries(motion.motion()).items()
    }
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


def test_a_step_out_of_arenas_case_fails_unless_waived() -> None:
    reference = motion.motion()
    resolves = "permanent resolves"
    bounds = motion.bounds(
        reference, motion.entries(reference)["fly"], "peak", resolves
    )
    assert bounds is not None
    big = motion.Seen(travels=True, half=0.65, start=bounds[1] + 1, peak=bounds[1] + 1)
    entry = motion.entries(reference)["fly"]
    entry["waivers"] = []
    out = motion.capture_verdicts(reference, "fly", resolves, big)
    assert [(v.measure, v.failed) for v in out if v.fault] == [("peak", True)]
    entry["waivers"] = [
        {
            "measure": "peak",
            "case": resolves,
            "bead": "by-w8gv.21.19",
            "reason": "a test",
        }
    ]
    out = motion.capture_verdicts(reference, "fly", resolves, big)
    assert not any(v.failed for v in out)
    # A flight that does not ease, or a token that flies in, is out.
    even = motion.Seen(travels=True, half=0.5)
    out = motion.capture_verdicts(reference, "fly", "own spell cast", even)
    assert [v.measure for v in out if v.failed] == ["curve"]
    flown = motion.Seen(travels=True, half=0.6)
    out = motion.capture_verdicts(reference, "appear", "token", flown)
    assert [v.measure for v in out if v.failed] == ["travel"]
    # A case with no curve taken is held to the verb's; a verb with none, to its sizes alone.
    out = motion.capture_verdicts(reference, "fly", "discard", even)
    assert [v.measure for v in out if v.failed] == ["curve"]
    assert motion.capture_verdicts(reference, "strike", None, even) == []
    lost = motion.Seen(travels=True, failure="2 things moved")
    out = motion.capture_verdicts(reference, "emerge", "triggered ability", lost)
    assert [v.measure for v in out if v.failed] == ["tracked"]


WIDTH, HEIGHT, FPS = 240, 135, 60
CARD_W, CARD_H = 30, 42


def flight(
    ease: Callable[[float], float], scale_from: float, length: float = 0.75
) -> Clip:
    """Frames of one card crossing a noisy table in 0.45 s from 0.1 s, eased by `ease` and
    growing (or shrinking) from `scale_from` of its size to it. A growing one is a copy, as an
    emerging ability is: it is not there before it leaves."""
    rng = random.Random(3)
    table = [rng.randint(30, 100) for _ in range(WIDTH * HEIGHT)]
    times = [n / FPS for n in range(round(length * FPS) + 1)]
    frames: list[bytes] = []
    for t in times:
        p = ease(min(1.0, max(0.0, (t - 0.1) / 0.45)))
        pixels = list(table)
        if scale_from < 1 and t < 0.1:
            frames.append(bytes(pixels))
            continue
        scale = scale_from + (1 - scale_from) * p
        w, h = round(CARD_W * scale), round(CARD_H * scale)
        cx, cy = 40 + 160 * p, 100 - 60 * p
        left, top = round(cx - w / 2), round(cy - h / 2)
        for y in range(max(0, top), min(HEIGHT, top + h)):
            for x in range(max(0, left), min(WIDTH, left + w)):
                pixels[y * WIDTH + x] = 200
        frames.append(bytes(pixels))
    return Clip(WIDTH, HEIGHT, times, frames, (WIDTH, HEIGHT))


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


def test_a_card_that_comes_to_be_in_place_does_not_travel() -> None:
    landed = flight(lambda p: p, 1.0)
    absent = bytes(landed.frames[0][:0]) + bytes(
        random.Random(3).randint(30, 100) for _ in range(WIDTH * HEIGHT)
    )
    frames = [absent] * 3 + [landed.frames[-1]] * 3
    clip = Clip(WIDTH, HEIGHT, landed.times[:6], frames, (WIDTH, HEIGHT))
    assert motion.seen(clip) == motion.Seen(travels=False)
