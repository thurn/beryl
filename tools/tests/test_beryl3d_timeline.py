"""`just b3d timeline-parity`: comparing the two clients' timelines and the allowlist."""

from __future__ import annotations

from typing import Any

from beryl_tools.beryl3d import timeline


def step(
    start: float = 0.0, beat: int = 0, number: int = 0, **extra: Any
) -> dict[str, Any]:
    return {
        "recording": "r",
        "seat": "you",
        "view": 2,
        "beat": beat,
        "step": number,
        "verb": "fly",
        "case": None,
        "start": start,
        "end": start + 100.0,
        "effects": [],
        **extra,
    }


def test_equal_timelines_agree() -> None:
    steps = [step(), step(100.0, number=1)]
    assert timeline.compare(steps, [dict(s) for s in steps]) is None


def test_two_milliseconds_apart_is_one_step_and_half_a_millisecond_is_none() -> None:
    assert timeline.compare([step(0.0)], [step(0.5)]) is None
    found = timeline.compare(
        [step(0.0), step(5.0, number=1)], [step(2.0), step(5.0, number=1)]
    )
    assert found is not None and found.count == 1


def test_a_step_one_side_lacks_counts_once() -> None:
    found = timeline.compare([step(), step(100.0, number=1)], [step()])
    assert found is not None and found.count == 1
    assert found.native is None and found.web is not None


def test_a_step_counts_once_however_many_fields_differ() -> None:
    found = timeline.compare([step()], [step(7.0, verb="tap", effects=["settle@end"])])
    assert found is not None and found.count == 1


def test_an_unlisted_differing_recording_fails() -> None:
    web = {"r/you": [step()]}
    native = {"r/you": [step(9.0)]}
    found = timeline.differences(web, native, {"r/you"})
    assert [d.key for d in timeline.over_limit(found, {})] == ["r/you"]
    assert timeline.over_limit(found, {"r/you": 1}) == []
    assert timeline.over_limit(found, {"r/you": 0}) != []


def test_update_lowers_an_entry_and_refuses_to_raise_one() -> None:
    web = {"a/you": [step(), step(1.0, number=1)], "b/you": [step()]}
    native = {"a/you": [step()], "b/you": [step(9.0)]}
    scope = {"a/you", "b/you"}
    found = timeline.differences(web, native, scope)
    allowed = {"a/you": 3, "b/you": 0, "outside/you": 4}
    entries, refused = timeline.lowered(found, allowed, scope, raise_=False)
    assert entries == {"a/you": 1, "outside/you": 4}
    assert refused == ["b/you"]
    entries, refused = timeline.lowered(found, allowed, scope, raise_=True)
    assert entries["b/you"] == 1 and refused == []
