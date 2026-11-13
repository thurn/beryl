"""`card render`: each normalization on the Oracle variation it absorbs, the comparison, and the
command on a small fake store with the renderer replaced."""

import argparse
import json
import subprocess
from pathlib import Path

import pytest

from beryl_tools import card, card_store, render_diff
from beryl_tools.render_diff import Context, Face

PLAIN: Context = Context(())
JACE: Context = render_diff.context_of(
    "Jace, the Mind Sculptor", ["Jace, the Mind Sculptor"]
)


def test_strip_reminder_text_keeps_a_line_of_reminder_text_alone() -> None:
    text = "Flash (You may cast this spell any time you could cast an instant.)\n({T}: Add {U}.)"
    assert render_diff.strip_reminder_text(text, PLAIN) == "Flash\n{T}: Add {U}."


def test_unify_self_reference_covers_the_name_its_short_form_and_this_type() -> None:
    text = (
        "Exile Jace, the Mind Sculptor. Jace deals 1 damage. Sacrifice this creature."
    )
    assert render_diff.unify_self_reference(text, JACE) == (
        "Exile ~. ~ deals 1 damage. Sacrifice ~."
    )
    # A longer word that starts with the short name is not the card.
    assert render_diff.unify_self_reference("Jaces", JACE) == "Jaces"


def test_unify_punctuation_writes_minus_signs_quotes_and_em_dashes_plainly() -> None:
    text = "−3: You get an emblem with “Draw.”\nEvoke — Exile"
    assert render_diff.unify_punctuation(text, PLAIN) == (
        '-3: You get an emblem with "Draw."\nEvoke—Exile'
    )


def test_unify_whitespace_collapses_spaces_and_drops_empty_lines() -> None:
    assert render_diff.unify_whitespace("Flying  \n\n  Draw  a card. ", PLAIN) == (
        "Flying\nDraw a card."
    )


def test_neuter_pronouns_writes_a_character_as_it() -> None:
    text = "Return him under his owner's control. Activate her loyalty abilities. Exile her."
    assert render_diff.neuter_pronouns(text, PLAIN) == (
        "Return it under its owner's control. Activate its loyalty abilities. Exile it."
    )


def test_share_spell_noun_says_spell_once_for_two_kinds() -> None:
    text = "Choose up to one target creature spell or planeswalker spell."
    assert render_diff.share_spell_noun(text, PLAIN) == (
        "Choose up to one target creature or planeswalker spell."
    )
    assert render_diff.share_spell_noun("Counter target spell.", PLAIN) == (
        "Counter target spell."
    )


def test_sort_mana_choices_puts_a_choice_in_color_order() -> None:
    assert render_diff.sort_mana_choices("{T}: Add {G}, {W}, or {U}.", PLAIN) == (
        "{T}: Add {W}, {U}, or {G}."
    )
    assert render_diff.sort_mana_choices("Add {U} or {W}.", PLAIN) == "Add {W} or {U}."
    # A cost is not a choice.
    assert render_diff.sort_mana_choices("{G}{W}: Draw.", PLAIN) == "{G}{W}: Draw."


def test_unify_mana_symbols_upper_cases_orders_hybrids_and_splits_runs() -> None:
    assert render_diff.unify_mana_symbols("{t}, {U/W}: {2}{W}", PLAIN) == (
        "{T}, {W/U}: {2} {W}"
    )


def test_lower_sentence_starts() -> None:
    text = "Draw a card. A spell costs {1} more. X can't be 0.\n~ gains flying."
    assert render_diff.lower_sentence_starts(text, PLAIN) == (
        "draw a card. a spell costs {1} more. X can't be 0.\n~ gains flying."
    )


def test_join_spell_paragraphs_only_on_a_spell_and_not_keyword_lines() -> None:
    text = "kicker {1}\nreturn target spell to its owner's hand.\ndraw a card."
    spell = Context((), spell=True)
    assert render_diff.join_spell_paragraphs(text, spell) == (
        "kicker {1}\nreturn target spell to its owner's hand. draw a card."
    )
    assert render_diff.join_spell_paragraphs(text, PLAIN) == text


def test_sort_keyword_lines_splits_and_sorts_a_run_of_keywords() -> None:
    text = "vigilance, flying\nflash\n~ deals 1 damage.\ncycling {3}"
    assert render_diff.sort_keyword_lines(text, PLAIN) == (
        "flash\nflying\nvigilance\n~ deals 1 damage.\ncycling {3}"
    )


def test_compare_matches_through_every_normalization() -> None:
    oracle = [
        Face(
            "Jace, the Mind Sculptor",
            "Flying, vigilance (Reminder.)\n−1: Return him to his owner's hand.",
        )
    ]
    rendered = [
        Face(
            "Jace, the Mind Sculptor",
            "Vigilance\nFlying\n-1: Return it to its owner's hand.",
        )
    ]
    assert render_diff.compare("Jace, the Mind Sculptor", oracle, rendered).matches


def test_compare_shows_each_differing_paragraph_as_a_word_diff() -> None:
    oracle = [Face("Elf", "Flying\nWhen this creature enters, you may play it.")]
    rendered = [Face("Elf", "Flying\nWhen this creature enters, you may cast it.")]
    diff = render_diff.compare("Elf", oracle, rendered)
    assert diff.lines == ["Elf: when ~ enters , you may [-play-] {+cast+} it ."]
    assert diff.causes == ["'play' -> 'cast'"]
    missing = render_diff.compare("Elf", oracle, [Face("Elf", "Flying")])
    assert missing.lines == ["Elf: [-when ~ enters , you may play it .-]"]
    assert render_diff.compare("Elf", oracle, []).causes == ["no rendered face"]


def store(tmp_path: Path) -> card.Sources:
    """A store of two creatures."""

    def creature(name: str, text: str) -> card_store.Json:
        face: card_store.Json = {
            "name": name,
            "type_line": "Creature",
            "oracle_text": text,
        }
        return {"name": name, "faces": [face]}

    cards = [
        creature("Elf", "Flying"),
        creature("Bird", "When this creature enters, draw a card."),
    ]
    path = tmp_path / "store.tsv"
    card_store.write(
        path,
        sorted(card_store.line("card", str(c["name"]), c) for c in cards),
    )
    return card.Sources(path, tmp_path / "vocabulary", tmp_path / "cards")


RENDERED = {
    "Elf": [Face("Elf", "Flying")],
    "Bird": [Face("Bird", "When this creature enters, draw two cards.")],
}


def renderer(names: list[str]) -> render_diff.Rendered:
    return [(n, RENDERED[n]) for n in (names or list(RENDERED))]


def run(tmp_path: Path, *names: str, all: bool = False) -> int:
    args = argparse.Namespace(names=list(names), all=all)
    return render_diff.run(args, store(tmp_path), renderer)


def test_render_prints_the_text_and_its_verdict(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert run(tmp_path, "elf") == 0
    assert capsys.readouterr().out == "== Elf\nElf\nFlying\n-- match\n"
    assert run(tmp_path, "Bird") == 1
    assert "[-a card-] {+two cards+}" in capsys.readouterr().out


def test_render_all_reports_the_match_rate_and_the_causes(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert run(tmp_path, all=True) == 1
    out = capsys.readouterr().out.splitlines()
    assert out[:2] == [
        "1 of 2 cards match their Oracle text",
        "'a card' -> 'two cards': Bird",
    ]


def test_render_refuses_a_stranger_and_no_names(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert run(tmp_path, "Elk Lord") == 1
    assert "no card named Elk Lord" in capsys.readouterr().err
    assert run(tmp_path) == 1
    assert "--all" in capsys.readouterr().err


def test_parse_reads_one_card_per_line_and_reports_a_failure() -> None:
    line = json.dumps({"name": "Elf", "faces": [{"name": "Elf", "text": "Flying"}]})
    ok = subprocess.CompletedProcess[str]([], 0, line + "\n", "")
    assert render_diff.parse(ok) == [("Elf", [Face("Elf", "Flying")])]
    failed = subprocess.CompletedProcess[str]([], 1, "", 'no card named "Elk"\n')
    assert render_diff.parse(failed) == 'no card named "Elk"'
