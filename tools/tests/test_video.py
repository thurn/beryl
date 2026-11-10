from pathlib import Path

from beryl_tools import video
from beryl_tools.video import Frame, Line


def stack(name: str, type_line: str, text: str = "") -> list[Line]:
    """The lines of the card Arena shows for the top of the stack."""
    lines = [Line(name, 1582, 287, 250, 23), Line(type_line, 1585, 533, 80, 17)]
    return lines + ([Line(text, 1591, 594, 280, 17)] if text else [])


def frames(*screens: list[Line]) -> list[Frame]:
    """Screens sampled twice a second."""
    return [Frame(n / 2, lines) for n, lines in enumerate(screens)]


def test_clean_name_drops_what_a_mana_cost_reads_as() -> None:
    assert (
        video.clean_name("Sheoldred, the Apocalypse 29C") == "Sheoldred, the Apocalypse"
    )
    assert video.clean_name("Mox Opal") == "Mox Opal"
    assert video.clean_name("Fire // Ice") == "Fire // Ice"


def test_clock_round_trips() -> None:
    assert video.clock(1472.5) == "24:32.5"
    assert video.clock(3723) == "1:02:03.0"
    assert video.clock(59.96) == "1:00.0"
    assert video.parse_clock("24:32.5") == 1472.5
    assert video.parse_clock("1:02:03") == 3723
    assert video.parse_clock("90") == 90


def test_video_id_is_the_bracketed_id_or_the_name() -> None:
    assert (
        video.video_id(Path("Cube Draft ⧸⧸ #443 [SswBO8aOOgc].webm")) == "SswBO8aOOgc"
    )
    assert video.video_id(Path("my game 1.mp4")) == "my-game-1"


def test_a_spell_then_its_own_trigger_are_two_events() -> None:
    spell = stack("Trinket Mage 2U", "Creature - Human Wizard")
    trigger = stack(
        "Trinket Mage", "Ability", "When this creature enters, you may search."
    )
    events = video.derive(frames(spell, spell, trigger, trigger, trigger, []))
    assert [(e.t, e.kind, e.name) for e in events] == [
        (0.0, "spell", "Trinket Mage"),
        (1.0, "ability", "Trinket Mage"),
    ]
    assert events[0].detail == "Creature - Human Wizard"
    assert events[1].detail == "When this creature enters, you may search."


def test_the_object_under_a_resolved_one_is_not_cast_again() -> None:
    bolt = stack("Lightning Bolt R", "Instant")
    counter = stack("Counterspell UU", "Instant")
    screens = [bolt, bolt, counter, counter, bolt, bolt, [], [], [], [], bolt, bolt]
    names = [(e.t, e.name) for e in video.derive(frames(*screens))]
    # The second Bolt comes after the stack emptied: a new spell.
    assert names == [
        (0.0, "Lightning Bolt"),
        (1.0, "Counterspell"),
        (5.0, "Lightning Bolt"),
    ]


def test_a_misread_name_stays_one_event() -> None:
    whole = stack("Thundering Falls", "Ability")
    part = stack("hundering Falls", "Ability")
    events = video.derive(frames(whole, part, whole, whole))
    assert [(e.kind, e.name, e.end) for e in events] == [
        ("ability", "Thundering Falls", 1.5)
    ]


def test_prompts_carry_their_instructions_and_buttons() -> None:
    surveil = [
        Line("Surveil", 865, 66, 190, 52),
        Line("Drag cards to the top of your library.", 660, 134, 600, 28),
        Line("20", 938, 148, 45, 31),
        Line("Done", 1700, 950, 80, 28),
    ]
    hover = [Line("Trinket Mage", 1139, 441, 150, 26)]
    events = video.prompt_events(frames(surveil, surveil, surveil, hover, hover))
    # The hovered card's name is not centered, so it is no prompt.
    assert [(e.name, e.detail, e.buttons) for e in events] == [
        ("Surveil", "Drag cards to the top of your library.", ["Done"])
    ]


def test_life_changes_once_two_samples_agree() -> None:
    def life(text: str, w: float = 45) -> list[Line]:
        return [Line(text, 938, 887, w, 36)]

    # "2036" is the total with mana in the pool beside it; "17" is read only once.
    screens = [
        life("20"),
        life("20"),
        life("2036", 110),
        life("17"),
        life("18"),
        life("18"),
    ]
    events = video.life_events(frames(*screens))
    assert [(e.t, e.name) for e in events] == [(2.5, "you 20 to 18")]


def test_draft_frames_carry_no_game() -> None:
    pick = [Line("Confirm Pick", 1250, 590, 140, 30), *stack("Mox Pearl", "Artifact")]
    events = video.derive(frames(pick, pick, pick))
    assert [(e.t, e.kind) for e in events] == [(0.0, "draft")]


def test_timeline_lines() -> None:
    event = video.Event(
        1472.0, 1473.0, "spell", "Sheoldred", "Legendary Creature", ["Resolve"]
    )
    assert video.timeline([event]) == (
        "  24:32.0  spell    Sheoldred | Legendary Creature [Resolve]\n"
    )
