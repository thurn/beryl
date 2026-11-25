"""The Phase translator accepts only the fields each builder consumes."""

import hashlib
import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, phase_data, phase_draft

FIXTURE: Path = Path(__file__).parent / "fixtures" / "phase-draft.json"


@pytest.fixture
def phase() -> dict[str, card_store.Json]:
    return cast(dict[str, card_store.Json], json.loads(FIXTURE.read_text()))


def test_translates_a_targeted_spell_and_an_enters_trigger(
    phase: dict[str, card_store.Json],
) -> None:
    spell = cast(list[object], phase["mind_rot"]["abilities"])[0]
    trigger = cast(list[object], phase["visionary"]["triggers"])[0]
    assert phase_draft.ability(spell).expression() == (
        "Ability::spell().target(&TargetSpec::player()).effect(&discard(target_player(), 2))"
    )
    assert phase_draft.triggered(trigger).expression() == "when_enters(&draw(you(), 1))"


def test_drafts_flamecast_wheel_with_a_permanent_damage_recipient() -> None:
    ability = phase_draft.ability(
        {
            "kind": "Activated",
            "effect": {
                "type": "DealDamage",
                "amount": {"type": "Fixed", "value": 3},
                "target": {
                    "type": "Typed",
                    "type_filters": ["Creature"],
                    "controller": None,
                    "properties": [],
                },
            },
            "cost": {
                "type": "Composite",
                "costs": [
                    {
                        "type": "Mana",
                        "cost": {"type": "Cost", "shards": [], "generic": 5},
                    },
                    {"type": "Tap"},
                    {"type": "Sacrifice", "target": {"type": "SelfRef"}, "count": 1},
                ],
            },
        }
    )
    assert ability.expression() == (
        'Ability::activated(Cost::of(mana("{5}"), &[CostPart::TAP, CostPart::SACRIFICE]))'
        ".target(&TargetSpec::creature())"
        ".effect(&Effect::DealDamage { to: Recipients::Permanents(target()), "
        "amount: Amount::Fixed(3) })"
    )


@pytest.mark.parametrize(
    ("target", "spec", "effect"),
    [
        ("Player", "player", "deal_damage_to(target_player(), 3)"),
        ("Opponent", "opponent", "deal_damage_to(target_player(), 3)"),
        ("Any", "any_target", "deal_damage_to_any_target(3)"),
    ],
)
def test_drafts_damage_to_player_or_any_target(
    target: str, spec: str, effect: str
) -> None:
    ability = phase_draft.ability(
        {
            "kind": "Spell",
            "effect": {
                "type": "DealDamage",
                "amount": {"type": "Fixed", "value": 3},
                "target": {"type": target},
            },
        }
    )
    assert ability.expression() == (
        f"Ability::spell().target(&TargetSpec::{spec}()).effect(&{effect})"
    )


def test_translates_an_optional_target_set_and_its_shared_keyword_grant(
    phase: dict[str, card_store.Json],
) -> None:
    spell = cast(list[object], phase["coordinated_assault"]["abilities"])[0]
    assert phase_draft.ability(spell).expression() == (
        "Ability::spell()"
        ".target(&TargetSpec::up_to(2, TargetKind::Permanent, Filter::Types(Types::CREATURE)))"
        ".effect(&modify(target(), &[Modification::ModifyPt(1, 0), "
        "Modification::AddKeywords(Keywords::FIRST_STRIKE)], Duration::EndOfTurn))"
    )


def test_translates_an_optional_target_set_without_a_sub_ability(
    phase: dict[str, card_store.Json],
) -> None:
    spell = cast(list[object], phase["dauntless_onslaught"]["abilities"])[0]
    assert phase_draft.ability(spell).expression() == (
        "Ability::spell()"
        ".target(&TargetSpec::up_to(2, TargetKind::Permanent, Filter::Types(Types::CREATURE)))"
        ".effect(&modify(target(), &[Modification::ModifyPt(2, 2)], Duration::EndOfTurn))"
    )


@pytest.mark.parametrize(
    "target_range",
    [
        {"min": 1, "max": 2},
        {"min": False, "max": 2},
        {"min": 0, "max": 3},
        {"max": 2},
        {"min": 0, "max": 2, "allow_repeat": True},
    ],
)
def test_refuses_unsupported_optional_target_ranges(
    phase: dict[str, card_store.Json], target_range: card_store.Json
) -> None:
    spell = cast(list[card_store.Json], phase["dauntless_onslaught"]["abilities"])[0]
    spell["multi_target"] = target_range
    with pytest.raises(phase_draft.Unsupported, match=r"ability\.multi_target"):
        phase_draft.ability(spell)


def test_refuses_a_second_target_in_the_pump_keyword_grant(
    phase: dict[str, card_store.Json],
) -> None:
    spell = cast(list[card_store.Json], phase["coordinated_assault"]["abilities"])[0]
    child = cast(card_store.Json, spell["sub_ability"])
    effect = cast(card_store.Json, child["effect"])
    effect["target"] = {"type": "Typed", "type_filters": ["Creature"]}
    with pytest.raises(
        phase_draft.Unsupported, match=r"pump sub_ability\.effect\.target"
    ):
        phase_draft.ability(spell)


def test_translates_a_life_loss_trigger_for_each_player(
    phase: dict[str, card_store.Json],
) -> None:
    trigger = cast(list[object], phase["blood-toll harpy"]["triggers"])[0]
    assert (
        phase_draft.triggered(trigger).expression()
        == "when_enters(&lose_life(each_player(), 1))"
    )


@pytest.mark.parametrize(
    ("overrides", "field"),
    [
        ({"player_scope": {"type": "Opponent"}}, "ability.player_scope"),
        ({"player_scope": {"type": "All", "extra": True}}, "ability.player_scope"),
        ({"effect": {"type": "GainLife"}}, "ability.player_scope"),
        (
            {"effect": {"type": "LoseLife", "target": {"type": "Player"}}},
            "effect.target",
        ),
        (
            {
                "sub_ability": {
                    "kind": "Spell",
                    "effect": {"type": "Draw", "count": {"type": "Fixed", "value": 1}},
                }
            },
            "ability.player_scope",
        ),
    ],
)
def test_life_loss_scope_rejects_unsupported_combinations(
    overrides: card_store.Json, field: str
) -> None:
    with pytest.raises(phase_draft.Unsupported, match=field):
        phase_draft.ability(
            {
                "kind": "Spell",
                "effect": {
                    "type": "LoseLife",
                    "amount": {"type": "Fixed", "value": 1},
                },
                "player_scope": {"type": "All"},
                **overrides,
            }
        )


@pytest.mark.parametrize(
    ("extra", "field"),
    [
        ({"unless_filter": {"type": "SelfRef"}}, "effect.unless_filter"),
        ({"destination": "Exile"}, "effect.destination"),
    ],
)
def test_extra_fields_in_nested_effects_stop_the_ability(
    phase: dict[str, card_store.Json], extra: card_store.Json, field: str
) -> None:
    ability = cast(list[card_store.Json], phase["mind_rot"]["abilities"])[0]
    effect = cast(card_store.Json, ability["effect"])
    effect.update(extra)
    with pytest.raises(phase_draft.Unsupported, match=field):
        phase_draft.ability(ability)


def test_a_mana_ability_cannot_drop_its_sub_ability() -> None:
    with pytest.raises(phase_draft.Unsupported, match="sub_ability"):
        phase_draft.ability(
            {
                "kind": "Activated",
                "effect": {
                    "type": "Mana",
                    "produced": {"type": "Fixed", "colors": ["Red"]},
                },
                "cost": {"type": "Tap"},
                "sub_ability": {"kind": "Activated", "effect": {"type": "Unsupported"}},
            }
        )


def test_cached_data_must_match_the_pinned_release(tmp_path: Path) -> None:
    pin = tmp_path / "pin.json"
    pin.write_text(
        json.dumps(
            {
                "card_data_hash": "example-phase-release",
                "sha256": hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
            }
        )
    )
    cached = tmp_path / "card-data.json"
    cached.write_text("{}")
    with pytest.raises(ValueError, match="example-phase-release"):
        phase_data.load(pin, cached)
    cached.write_bytes(FIXTURE.read_bytes())
    assert "mind_rot" in phase_data.load(pin, cached)


def test_a_target_the_parse_dropped_stops_the_draft() -> None:
    # Phase's parse of Inspiration: its Draw has no target, though the sentence names one.
    inspiration = {
        "kind": "Spell",
        "effect": {"type": "Draw", "count": {"type": "Fixed", "value": 2}},
        "cost": None,
        "sub_ability": None,
        "duration": None,
        "description": "Target player draws two cards.",
        "target_prompt": None,
        "sorcery_speed": False,
        "condition": None,
        "optional_targeting": False,
        "optional": False,
        "forward_result": False,
    }
    with pytest.raises(phase_draft.Unsupported, match="target the parse dropped"):
        phase_draft.ability(inspiration)
    divination = {**inspiration, "description": "Draw two cards."}
    assert phase_draft.ability(divination).expression() == (
        "Ability::spell().effect(&draw(you(), 2))"
    )
