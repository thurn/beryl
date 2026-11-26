"""`just ios-smoke --game`: what makes a finished game's run fail."""

from beryl_tools.client import ios_game

DONE = {"result": "You won (life)", "turns": 9, "faceless_cards": 0, "missing_glyphs": 0}


def test_a_finished_game_with_every_face_and_no_error_has_no_faults() -> None:
    assert ios_game.game_faults(DONE, True, "") == []


def test_each_missing_result_blank_card_error_and_exit_is_a_fault() -> None:
    assert ios_game.game_faults({}, True, "")[0].endswith("has no result")
    blank = ios_game.game_faults({**DONE, "faceless_cards": 2}, True, "")
    assert "2 cards blank" in blank[0]
    assert "wrote an error" in ios_game.game_faults(DONE, True, "panic")[0]
    assert "no longer running" in ios_game.game_faults(DONE, False, "")[0]


def test_the_final_summary_is_the_last_one_with_a_result() -> None:
    log = '{"summary": {"frames": 1}}\n{"summary": {"result": "Draw", "frames": 2}}\n'
    assert ios_game.final_summary(log)["frames"] == 2
