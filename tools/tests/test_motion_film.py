from typing import Any

from beryl_tools import motion_film


def instance(**over: Any) -> dict[str, Any]:
    return {
        "case": "triggered ability",
        "recording": "abc",
        "time": "20:39.15",
        "span": 0.35,
        "crop": [850, 250, 700, 500],
        "command": "just video film abc 20:39.15 --span 0.35 --fps 30 --cols 4"
        " --crop 850,250,700,500  then 20:39.45 span 0.6",
        "duration_ms": 300,
        "delay_ms": None,
        **over,
    }


def test_an_instance_is_retaken_as_its_command_took_it_past_where_it_rests() -> None:
    take = motion_film.retake(instance())
    assert (take.recording, take.crop) == ("abc", "850,250,700,500")
    assert take.start == 20 * 60 + 39.15
    # The command's first cut ends before the motion does: the span reaches past it.
    assert take.span == 0.3 + motion_film.SETTLE
    # A longer cut is kept, and the tracker's command is read the same.
    long = "just video measure abc 52:54.4 --span 1.6 --crop 1,2,3,4 --json"
    assert motion_film.retake(instance(command=long)).span == 1.6
    # With no command to read, the record's own time, span and crop.
    bare = motion_film.retake(instance(command="by eye", crop=None, span=2))
    assert (bare.start, bare.span, bare.crop) == (20 * 60 + 39.15, 2, None)


def test_the_first_instances_of_each_case_are_filmed_or_of_the_case_asked_for() -> None:
    cases = ["draw", "draw", "draw", "discard"]
    reference = {
        "verbs": [
            {
                "verb": "fly",
                "instances": [instance(case=c, time=n) for n, c in enumerate(cases)],
            }
        ]
    }
    found = motion_film.chosen(reference, "fly", None, 2)
    assert [(i["case"], i["time"]) for i in found] == [
        ("draw", 0),
        ("draw", 1),
        ("discard", 3),
    ]
    assert [i["time"] for i in motion_film.chosen(reference, "fly", "discard", 2)] == [
        3
    ]
    assert motion_film.chosen(reference, "strike", None, 2) == []
