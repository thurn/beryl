"""`just b3d stand-in`: reading the live tree, choosing an answer and comparing boards."""

import pytest

from beryl_tools.beryl3d import stand_in

DISCARD = """t=33818ms
game
  table
    card "Plains" #35 hand.yours 666,726 184x255 interactive eligible covered(54%)
    card "Island" #37 hand.yours 824,724 197x265 interactive eligible hovered
    card #261 hand.theirs 490,-115 115x153 face-down covered(4%)
  layer
    region "game" #65604 1168,733 393x80
      region "prompt" #65636 1168,733 393x80
        text "Discard a card." #65637 1184,749 159x48
        button "Submit 0" #65638 1353,749 192x48 interactive disabled
    player "You 20" #65598 786,659 28x30
    player "Opponent 20" #65600 786,89 28x30
    text "Your library 32" #65673 61,764 18x22
    button "Settings" #65597 1544,12 44x44 interactive
"""


def test_a_tree_gives_its_clock_and_each_element_with_its_place_and_states() -> None:
    tree = stand_in.parse_tree(DISCARD)
    assert tree.clock_ms == 33818
    plains = tree.elements[2]
    assert plains.locator == 'card "Plains" #35'
    assert plains.place == "hand.yours"
    assert plains.states == {"interactive", "eligible", "covered"}
    assert tree.elements[4].locator == "card #261"
    prompt = tree.region("prompt")
    assert prompt is not None
    assert [e.role for e in prompt] == ["text", "button"]


def test_a_tree_without_its_clock_line_is_refused() -> None:
    with pytest.raises(stand_in.StandInError, match="no clock line"):
        stand_in.parse_tree("game\n  layer\n")


def test_with_every_button_disabled_the_first_eligible_card_is_chosen() -> None:
    answer = stand_in.choose(stand_in.parse_tree(DISCARD))
    assert answer is not None
    assert answer.locator == 'card "Plains" #35'


def test_the_enabled_primary_button_comes_before_the_others() -> None:
    tree = stand_in.parse_tree(
        DISCARD.replace(
            '        button "Submit 0" #65638 1353,749 192x48 interactive disabled\n',
            '        button "Mulligan" #2 1353,749 192x48 interactive\n'
            '        button "Keep 7" #3 1151,749 192x48 interactive primary\n',
        )
    )
    answer = stand_in.choose(tree)
    assert answer is not None and answer.name == "Keep 7"


def test_with_no_prompt_there_is_no_answer_and_settings_is_never_pressed() -> None:
    tree = stand_in.parse_tree(
        "\n".join(line for line in DISCARD.splitlines() if "prompt" not in line)
    )
    assert stand_in.choose(tree) is None


def test_the_prompt_key_ignores_the_pointer_but_not_a_new_answer() -> None:
    before = stand_in.parse_tree(DISCARD)
    hovered = stand_in.parse_tree(DISCARD.replace("disabled\n", "disabled hovered\n"))
    assert stand_in.prompt_key(before) == stand_in.prompt_key(hovered)
    picked = stand_in.parse_tree(
        DISCARD.replace('"Submit 0" #65638', '"Submit 1" #65639').replace(
            "interactive disabled", "interactive primary"
        )
    )
    assert stand_in.prompt_key(before) != stand_in.prompt_key(picked)


def test_the_board_counts_life_piles_and_cards_by_place_not_position() -> None:
    board = stand_in.board(stand_in.parse_tree(DISCARD))
    assert board["player You 20"] == 1
    assert board["text Your library 32"] == 1
    assert board["card Plains hand.yours"] == 1
    assert board["card #261 hand.theirs"] == 1
    assert "text Discard a card." not in board
    moved = stand_in.parse_tree(DISCARD.replace("666,726", "600,700"))
    assert stand_in.board(moved) == board
    hurt = stand_in.parse_tree(DISCARD.replace('"You 20"', '"You 17"'))
    assert stand_in.board(hurt) != board
