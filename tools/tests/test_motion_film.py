from typing import Any

from beryl_tools import conventions, motion, motion_film


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


def pose(at: float, x: float, y: float, width: float = 63.0) -> dict[str, Any]:
    return {
        "at": at,
        "x": x,
        "y": y,
        "width": width,
        "rotation": 0,
        "turn": 0,
        "opacity": 1,
        "depth": 900,
        "face": "front",
    }


def test_a_steps_poses_are_told_along_its_path_with_its_bow() -> None:
    # Half way in time it is three quarters of the way along, lifted 20 of the path's 200.
    step = {
        "from": 0,
        "to": 100,
        "poses": [{"at": -17}, pose(0, 0, 100), pose(50, 150, 80), pose(100, 200, 100)],
    }
    lines = motion_film.pose_lines(step, 17)
    assert lines[0] == "  the path's bow 0.1 of its chord (bows up)"
    assert "along" in lines[1]
    assert lines[3].split()[:4] == ["+50", "0.750", "0.100", "1.00x"]
    # A card that changes where it lies is told where, and one the stage has no pose of is not.
    still = {"from": 0, "to": 33, "poses": [pose(0, -21.5, -24, 126), pose(33, 10, 20)]}
    lines = motion_film.pose_lines(still, 17)
    assert lines[0].split()[:3] == ["ms", "x", "y"] and lines[1].split()[3] == "2.00x"
    assert (
        "no card"
        in motion_film.pose_lines({"from": 0, "to": 9, "poses": [{"at": 0}]}, 17)[0]
    )


def test_the_brief_gives_a_reviewer_the_prose_the_values_and_every_sheet() -> None:
    data = conventions.load()
    reference = motion.motion()
    take = {
        "layout": "phone",
        "seat": "yours",
        "name": "cast-10-triggered-ability",
        "fixture": "combat-declare",
        "from": 1000,
        "to": 1275,
        "sheets": {
            "table": ["/t.png"],
            "strip": ["/s.png", "/s-2.png"],
            "ticks": ["/k.png"],
        },
        "steps": [
            {
                "n": 10,
                "case": "triggered ability",
                "from": 0,
                "to": 275,
                "act": {"verb": "emerge"},
                "rest": [0.5, 0.5],
                "poses": [pose(0, 0, 0), pose(275, 300, 0)],
            }
        ],
        "during": [{"verb": "point", "case": None, "from": 220, "to": 253}],
    }
    arena = [
        {
            "label": "triggered ability · abc 20:39.15",
            "instance": instance(
                note="Copy first visible 20:39.38.", method="filmstrip"
            ),
            "tracker": "cannot follow it (2 things moved): read the contact sheet",
        }
    ]
    filmed = {
        "tick": 16.7,
        "takes": [take],
        "contacts": [
            {"instance": arena[0]["label"], "seat": "yours", "path": "/c.png"}
        ],
    }
    text = motion_film.brief(
        data, reference, "emerge", None, filmed, arena, ["x: gone"]
    )
    assert text.startswith("MOTION REVIEW: emerge\n")
    for part in (
        "Shows: An ability lifts out of its source",
        "Arena's path: A curve from the source to the stack",
        "emerge (triggered ability). Beryl: 275 ms",
        "note: Copy first visible 20:39.38.",
        "contact sheet: /c.png",
        "left out: x: gone",
        "phone / yours / cast-10-triggered-ability   (fixture combat-declare, 275 ms)",
        "steps: emerge (triggered ability) +0 to +275",
        "playing meanwhile: point +220 to +253",
        "strip: /s-2.png",
    ):
        assert part in text, part
    # One case asked for leaves the other cases' values out.
    one = motion_film.brief(
        data, reference, "emerge", "triggered ability", filmed, arena, []
    )
    assert "emerge (activated ability)" not in one and "emerge. Beryl" in one


def test_a_verbs_fixtures_are_its_moments() -> None:
    assert motion_film.fixtures_of("emerge") == ["combat-declare", "verbs-cast-theirs"]
    assert "verbs-combat-theirs" in motion_film.fixtures_of("strike")
