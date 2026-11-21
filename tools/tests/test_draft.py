"""Drafts cover Oracle clauses in order and can borrow a rendered ability."""

import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, draft, phase_draft, render_diff


def record(name: str, text: str, *, type_line: str = "Instant") -> card_store.Json:
    return {
        "name": name,
        "layout": "normal",
        "faces": [{"name": name, "type_line": type_line, "oracle_text": text}],
    }


def opt() -> card_store.Json:
    fixture = Path(__file__).parent / "fixtures" / "phase-draft.json"
    return cast(card_store.Json, json.loads(fixture.read_text())["opt"])


def test_phase_spell_clauses_render_as_one_spell_ability() -> None:
    proposal = draft.compose(record("Opt", "Scry 1.\nDraw a card."), opt(), tuple)
    assert proposal.complete
    assert (
        "Ability::spell().effect(&sequence(&[scry(you(), 1), draw(you(), 1)]))"
        in proposal.expression
    )


def test_one_spell_clause_can_come_from_a_matching_rendered_ability() -> None:
    phase = opt()
    phase["abilities"] = cast(list[object], phase["abilities"])[:1]
    cantrip = draft.Candidate(
        "Draw a card.", phase_draft.Ability(draft.SPELL, ("draw(you(), 1)",)), "Wisps"
    )
    proposal = draft.compose(
        record("Opt", "Scry 1. Draw a card."), phase, lambda: (cantrip,)
    )
    assert proposal.complete
    assert "sequence(&[scry(you(), 1), draw(you(), 1)])" in proposal.expression


def test_fallback_excludes_the_card_being_drafted() -> None:
    self_match = draft.Candidate(
        "Draw a card.", phase_draft.Ability(draft.SPELL, ("draw(you(), 1)",)), "Opt"
    )
    proposal = draft.compose(record("Opt", "Draw a card."), None, lambda: (self_match,))
    assert not proposal.complete
    assert tuple(c.text for c in proposal.unresolved) == ("draw a card.",)


def test_outdated_oracle_text_cannot_supply_a_draft() -> None:
    proposal = draft.compose(record("Opt", "Scry 2.\nDraw a card."), opt(), tuple)
    assert not proposal.expression
    assert "Oracle texts differ" in proposal.source_note


@pytest.mark.parametrize(
    ("first_ability", "matches"),
    [
        ("+2: Draw a card.", True),
        ("+3: Draw a card.", False),
        ("+2: Draw two cards.", False),
    ],
    ids=["equivalent-prefixes", "changed-cost", "changed-effect"],
)
def test_phase_loyalty_cost_brackets(first_ability: str, matches: bool) -> None:
    abilities = [
        {
            "kind": "Spell" if amount is None else "Activated",
            "description": description,
            "cost": (
                {"type": "Loyalty", "amount": amount} if amount is not None else None
            ),
            "effect": {"type": "Unimplemented"},
        }
        for description, amount in (
            ("[+2]: Draw a card.", 2),
            ("[−X]: Draw X cards.", None),
            ("[0]: Scry 1.", 0),
            ("[−10]: Draw ten cards.", -10),
        )
    ]
    phase: card_store.Json = {
        "oracle_text": "\n".join(str(ability["description"]) for ability in abilities),
        "abilities": abilities,
    }
    oracle = f"{first_ability}\n−X: Draw X cards.\n0: Scry 1.\n−10: Draw ten cards."
    proposal = draft.compose(
        record("Example", oracle, type_line="Planeswalker"), phase, tuple
    )
    if matches:
        assert not proposal.source_note
        assert tuple(clause.node_kind for clause in proposal.unresolved) == (
            "cost:Loyalty",
            "",
            "cost:Loyalty",
            "cost:Loyalty",
        )
        assert "unsupported Unimplemented" in proposal.unresolved[1].reason
    else:
        assert "Oracle texts differ" in proposal.source_note
        assert all(not clause.node_kind for clause in proposal.unresolved)


def test_portable_spells_keep_their_target_expression() -> None:
    source = """
use crate::prelude::*;

pub static CARD: CardDef = Printed::Counterspell
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&counter(target()))])
    .card();
"""
    translated = draft.portable_ability(source, spell=True)
    assert translated == phase_draft.Ability(
        draft.SPELL, ("counter(target())",), "TargetSpec::spell()"
    )
    local = source.replace("counter(target())", "LOCAL_EFFECT")
    assert draft.portable_ability(local, spell=True) is None


def test_render_status_marks_each_sentence_and_highlights_missing_words() -> None:
    output = draft.render_status(
        record("Opt", "Scry 1.\nDraw a card."),
        [("Opt", [render_diff.Face("Opt", "Scry 1.\nDraw two cards.")])],
    )
    assert "Match · scry 1." in output
    assert "Diff · draw [-a card-] {+two cards+}" in output


def test_unread_casting_cost_leaves_a_draft_for_manual_review() -> None:
    phase = opt()
    phase["additional_cost"] = {"type": "Required"}
    proposal = draft.compose(record("Opt", "Scry 1.\nDraw a card."), phase, tuple)
    assert proposal.expression
    assert not proposal.complete
    assert any("card.additional_cost" in issue.reason for issue in proposal.unresolved)
