from beryl_tools import video_cards
from beryl_tools.video import Event, Frame, Line
from beryl_tools.video_cards import Card, Names, Recording, Sighting


def hand(name: str) -> Line:
    return Line(name, 700, 955, 150, 20)


def recording() -> Recording:
    """Teferi sits in hand for three samples, is cast, and its ability asks for a target."""
    held = [hand("Teferi, Time Raveler"), Line("Next", 1700, 950, 60, 28)]
    frames = [Frame(n / 2, held if n < 3 else []) for n in range(12)]
    events = [
        Event(0.0, 0.0, "turn", "YOUR TURN"),
        Event(
            2.0, 2.5, "spell", "Teferi, Time Raveler", "Legendary Planeswalker - Teferi"
        ),
        Event(4.0, 5.0, "ability", "Teferi, Tlme Raveler", "Draw a card.", ["Cancel"]),
        Event(4.5, 5.0, "prompt", "Target up to one creature.", buttons=["Cancel"]),
        Event(30.0, 30.0, "life", "you 20 to 18"),
    ]
    events[1].shot = "shots/0-02.5.jpg"
    return Recording("abc", frames, events)


def test_names_match_misreadings_of_long_names_only() -> None:
    names = Names(["Teferi, Time Raveler", "Oust", "Next"], fuzzy=True)
    assert names.match("Teferi, Tlme Raveler 1WU") == "Teferi, Time Raveler"
    assert names.match("oust") == "Oust"
    assert names.match("Ousts") == ""
    assert (
        Names(["Teferi, Time Raveler"], fuzzy=False).match("Teferi, Tlme Raveler") == ""
    )


def test_find_counts_stack_events_and_merges_sightings() -> None:
    found = video_cards.find(
        [recording()], Names(["Teferi, Time Raveler", "Next"], True)
    )
    # "Next" is a card, but here it is the button.
    assert list(found) == ["Teferi, Time Raveler"]
    card = found["Teferi, Time Raveler"]
    assert (card.count("spell"), card.count("ability")) == (1, 1)
    assert card.seen == [Sighting("abc", "hand", 0.0, 1.0)]
    assert card.videos() == {"abc"}


def test_table_has_a_row_per_card() -> None:
    card = video_cards.find([recording()], Names(["Teferi, Time Raveler"], True))
    rows = video_cards.table([*card.values(), Card("Oust")]).splitlines()
    assert rows[1].split()[-6:] == ["1", "1", "1", "0", "0", "1"]
    assert rows[2].split() == ["Oust", "0", "0", "0", "0", "0", "0"]


def test_dump_numbers_stack_events_with_what_follows() -> None:
    found = video_cards.find([recording()], Names(["Teferi, Time Raveler"], True))
    text = video_cards.dump(found["Teferi, Time Raveler"], [recording()])
    lines = text.splitlines()
    assert lines[0] == "# Teferi, Time Raveler"
    assert "type line: Legendary Planeswalker - Teferi" in lines
    assert "ability a (1x): Draw a card." in lines
    assert "#1 0:02.0 cast  0s  your turn  shots/0-02.5.jpg" in lines
    assert (
        "    +2s ability Teferi, Tlme Raveler; +2s Target up to one creature. [Cancel]"
        in lines
    )
    assert "#2 0:04.0 ability a  1s  your turn  [Cancel]" in lines
    assert "    +0s Target up to one creature. [Cancel]" in lines
    assert lines[-1] == "hand: 0:00.0-0:01.0"


def test_spans_join_sightings_close_together() -> None:
    seen = [
        Sighting("a", "hand", 0, 5),
        Sighting("a", "hand", 20, 20),
        Sighting("a", "hand", 90, 90),
    ]
    assert video_cards.spans(seen) == "0:00.0-0:20.0, 1:30.0"


def test_catalog_is_fetched_once_and_split_into_faces(tmp_path, monkeypatch) -> None:  # type: ignore[no-untyped-def]
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    asked: list[str] = []

    def get(url: str) -> dict[str, object]:
        asked.append(url)
        return {"data": ["Oust", "Fire // Ice"]}

    assert video_cards.catalog(get) == ["Fire", "Ice", "Oust"]
    assert video_cards.catalog(get) == ["Fire", "Ice", "Oust"]
    assert asked == [video_cards.CATALOG]
