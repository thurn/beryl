//! A template of each part against the Oracle text of a card it renders: an effect, a cost,
//! a trigger, a static ability and a target's filter. Each definition here is written the
//! way that card's file writes it; the expected text is the card's Oracle text.

use super::{render, render_ability};
use crate::ability::{Ability, Behavior, Custom, Says};
use crate::characteristics::{Supertypes, Types};
use crate::continuous::{Governed, RuleChange, When};
use crate::cost::{Cost, CostPart};
use crate::definition::{CardDef, Face, card};
use crate::effect::build::{
    IF_YOU_LOSE_THE_FLIP, TARGET, YOU, counter, deal_damage_to, destroy, draw,
    exile_until_this_leaves, flip_coin, if_then, put_back, sequence,
};
use crate::effect::{Effect, Players};
use crate::filter::Filter;
use crate::ids::TurnStep;
use crate::mana::{ManaSet, mana};
use crate::target::{TargetKind, TargetSpec};
use crate::trigger::Trigger;

/// A one-face card named `name`.
const fn one_face(name: &'static str, face: &'static [Face]) -> CardDef {
    card(name, face)
}

/// An effect: Jace, the Mind Sculptor's "0: Draw three cards, then put two cards from your
/// hand on top of your library in any order."
#[test]
fn an_effect_renders_as_its_oracle_sentence() {
    static BRAINSTORM: Effect = sequence(&[draw(YOU, 3), put_back(YOU, 2)]);
    static JACE: CardDef = one_face(
        "Jace, the Mind Sculptor",
        &[Face::EMPTY
            .types(Types::PLANESWALKER)
            .supertypes(Supertypes::LEGENDARY)
            .loyalty(3)
            .abilities(&[Ability::loyalty(0).effect(&BRAINSTORM)])],
    );
    assert_eq!(
        render(&JACE),
        "0: Draw three cards, then put two cards from your hand on top of your library in any \
         order."
    );
}

/// A cost: Strip Mine's "{T}: Add {C}." and "{T}, Sacrifice this land: Destroy target land."
#[test]
fn a_cost_renders_before_its_colon() {
    static STRIP_MINE: CardDef = one_face(
        "Strip Mine",
        &[Face::EMPTY.types(Types::LAND).abilities(&[
            Ability::tap_for_mana(ManaSet::one(crate::ManaColor::Colorless)),
            Ability::activated(Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]))
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::Types(Types::LAND),
                )])
                .effect(&destroy(TARGET)),
        ])],
    );
    assert_eq!(
        render(&STRIP_MINE),
        "{T}: Add {C}.\n{T}, Sacrifice this land: Destroy target land."
    );
}

/// A trigger: Mana Crypt's "At the beginning of your upkeep, flip a coin. If you lose the
/// flip, this artifact deals 3 damage to you."
#[test]
fn a_trigger_leads_its_effect() {
    static UPKEEP: Trigger = Trigger::step(TurnStep::Upkeep, Players::You);
    static FLIP: Effect = sequence(&[
        flip_coin(YOU),
        if_then(IF_YOU_LOSE_THE_FLIP, &deal_damage_to(YOU, 3)),
    ]);
    static MANA_CRYPT: CardDef = one_face(
        "Mana Crypt",
        &[Face::EMPTY
            .types(Types::ARTIFACT)
            .abilities(&[Ability::triggered(&UPKEEP).effect(&FLIP)])],
    );
    assert_eq!(
        render(&MANA_CRYPT),
        "At the beginning of your upkeep, flip a coin. If you lose the flip, this artifact \
         deals 3 damage to you."
    );
}

/// A static ability: Teferi, Time Raveler's "Each opponent can cast spells only any time
/// they could cast a sorcery."
#[test]
fn a_static_ability_says_what_it_changes() {
    static SORCERY_SPEED: RuleChange = RuleChange {
        players: Players::EachOpponent,
        what: Governed::CastSpells(Filter::Any),
        when: When::OnlyAsSorcery,
    };
    assert_eq!(
        render_ability(&Ability::rule_change(&SORCERY_SPEED)),
        "Each opponent can cast spells only any time they could cast a sorcery."
    );
}

/// A target's filter: Portable Hole's "When this artifact enters, exile target nonland
/// permanent an opponent controls with mana value 2 or less until this artifact leaves the
/// battlefield."
#[test]
fn a_filter_renders_as_a_noun_phrase() {
    static HOLE: Effect = exile_until_this_leaves(TARGET);
    static PORTABLE_HOLE: CardDef = one_face(
        "Portable Hole",
        &[Face::EMPTY
            .types(Types::ARTIFACT)
            .abilities(&[Ability::triggered(&Trigger::ENTERS)
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[
                        Filter::Not(&Filter::Types(Types::LAND)),
                        Filter::Whose(&Players::EachOpponent),
                        Filter::ManaValueAtMost(2),
                    ]),
                )])
                .effect(&HOLE)])],
    );
    assert_eq!(
        render(&PORTABLE_HOLE),
        "When this artifact enters, exile target nonland permanent an opponent controls with \
         mana value 2 or less until this artifact leaves the battlefield."
    );
}

/// An ability with no card to read refers to itself generically, and a Rust body, which
/// has no text, renders as a marked placeholder.
#[test]
fn a_bare_ability_and_a_rust_body() {
    static COUNTER: Effect = counter(TARGET);
    static BODY: fn() = || {};
    static A_SPELL: [TargetSpec; 1] = [TargetSpec::one(TargetKind::Spell, Filter::Any)];
    let counterspell = Ability::spell().targeting(&A_SPELL).effect(&COUNTER);
    assert_eq!(render_ability(&counterspell), "Counter target spell.");
    let custom = Ability::activated(Cost::mana(mana("{1}")))
        .custom(Behavior::Custom(Custom::new(&BODY, &Says::NOTHING)));
    assert_eq!(render_ability(&custom), "{1}: [custom ability].");
}
