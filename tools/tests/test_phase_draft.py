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
