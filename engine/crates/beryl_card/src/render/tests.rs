//! A template of each part against the Oracle text of a card it renders: an effect, a cost,
//! a trigger, a static ability and a target's filter. Each definition here is written the
//! way that card's file writes it; the expected text is the card's Oracle text.

use super::{render, render_ability};
use crate::CounterKind;
use crate::ability::{Ability, Behavior, Custom, Says};
use crate::characteristics::{Supertypes, Types};
use crate::continuous::{Governed, RuleChange, When};
use crate::cost::{Cost, CostPart};
use crate::definition::{CardDef, Face, card};
use crate::effect::build::may;
use crate::effect::build::{
    carried_object, counter, deal_damage_to, deal_damage_to_any_target, delay_with, destroy, dig,
    discard, draw, exile, exile_until_this_leaves, flip_coin, if_then, if_you_lose_the_flip,
    may_cast_free, permanents, put_any_on_bottom, put_back, return_to_battlefield, sequence,
    target, this, those, you,
};
use crate::effect::{Amount, Comparison, Condition, Effect, LibraryOrder, Objects, Players, Taken};
use crate::filter::Filter;
use crate::ids::TurnStep;
use crate::mana::{ManaSet, mana};
use crate::phrases::creature_you_control;
use crate::target::{TargetKind, TargetSpec};
use crate::trigger::{Subject, Trigger, TriggerEvent};

/// A one-face card named `name`.
const fn one_face(name: &'static str, face: &'static [Face]) -> CardDef {
    card(name, face)
}

/// An effect: Jace, the Mind Sculptor's "0: Draw three cards, then put two cards from your
/// hand on top of your library in any order."
#[test]
fn an_effect_renders_as_its_oracle_sentence() {
    static BRAINSTORM: Effect = sequence(&[draw(you(), 3), put_back(you(), 2)]);
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

#[test]
fn any_number_from_hand_and_a_draw_of_that_many_plus_one_render_as_one_sentence() {
    static EFFECT: Effect = sequence(&[
        put_any_on_bottom(you()),
        Effect::Draw {
            player: you(),
            count: Amount::Plus(1, &Amount::ThatMany),
        },
    ]);
    assert_eq!(
        render_ability(&Ability::spell().effect(&EFFECT)),
        "Put any number of cards from your hand on the bottom of your library, then draw that \
         many cards plus one."
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
                .effect(&destroy(target())),
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
        flip_coin(you()),
        if_then(if_you_lose_the_flip(), &deal_damage_to(you(), 3)),
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

/// A companion ability and a permission to cast from a graveyard once each turn: Lurrus of the
/// Dream-Den's "Companion — Each permanent card in your starting deck has mana value 2 or
/// less." and "Once during each of your turns, you may cast a permanent spell with mana value
/// 2 or less from your graveyard."
#[test]
fn a_companion_condition_and_a_graveyard_permission() {
    use crate::ability::DeckCondition;
    const PERMANENT: Filter = Filter::Types(Types::PERMANENT);
    static CONDITION: DeckCondition = DeckCondition {
        each: PERMANENT,
        has: Filter::ManaValueAtMost(2),
    };
    static SMALL: Filter = Filter::All(&[PERMANENT, Filter::ManaValueAtMost(2)]);
    assert_eq!(
        render_ability(&Ability::companion(&CONDITION)),
        "Companion — Each permanent card in your starting deck has mana value 2 or less."
    );
    assert_eq!(
        render_ability(&Ability::cast_from_graveyard_each_turn(&SMALL)),
        "Once during each of your turns, you may cast a permanent spell with mana value 2 or \
         less from your graveyard."
    );
}

/// A target's filter: Portable Hole's "When this artifact enters, exile target nonland
/// permanent an opponent controls with mana value 2 or less until this artifact leaves the
/// battlefield."
#[test]
fn a_filter_renders_as_a_noun_phrase() {
    static HOLE: Effect = exile_until_this_leaves(target());
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
    static COUNTER: Effect = counter(target());
    static BODY: fn() = || {};
    static A_SPELL: [TargetSpec; 1] = [TargetSpec::one(TargetKind::Spell, Filter::Any)];
    let counterspell = Ability::spell().targeting(&A_SPELL).effect(&COUNTER);
    assert_eq!(render_ability(&counterspell), "Counter target spell.");
    let custom = Ability::activated(Cost::mana(mana("{1}")))
        .custom(Behavior::Custom(Custom::new(&BODY, &Says::NOTHING)));
    assert_eq!(render_ability(&custom), "{1}: [custom ability].");
}

/// Two piles in one sentence: Stock Up's "Look at the top five cards of your library. Put two
/// of them into your hand and the rest on the bottom of your library in any order."
#[test]
fn a_dig_puts_both_piles_in_one_sentence() {
    static STOCK_UP: Effect = dig(you(), 5, Taken::Put(2), LibraryOrder::Any);
    assert_eq!(
        render_ability(&Ability::spell().effect(&STOCK_UP)),
        "Look at the top five cards of your library. Put two of them into your hand and the \
         rest on the bottom of your library in any order."
    );
}

/// A discarded card named after the object itself: Malcolm, Alluring Scoundrel's "Draw a
/// card, then discard a card. If there are four or more chorus counters on Malcolm, you may
/// cast the discarded card without paying its mana cost." Its "it" would mean Malcolm.
#[test]
fn a_discarded_card_is_named_once_it_would_be_ambiguous() {
    static LOOT: Effect = sequence(&[
        draw(you(), 1),
        discard(you(), 1),
        if_then(
            Condition::Compare(
                Amount::Counters(this(), CounterKind::Chorus),
                Comparison::AtLeast,
                Amount::Fixed(4),
            ),
            &may_cast_free(you(), those()),
        ),
    ]);
    static MALCOLM: CardDef = one_face(
        "Malcolm, Alluring Scoundrel",
        &[Face::EMPTY
            .types(Types::CREATURE)
            .supertypes(Supertypes::LEGENDARY)
            .abilities(&[
                Ability::triggered(&Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER).effect(&LOOT),
            ])],
    );
    assert_eq!(
        render(&MALCOLM),
        "Whenever Malcolm deals combat damage to a player, draw a card, then discard a card. If \
         there are four or more chorus counters on Malcolm, you may cast the discarded card \
         without paying its mana cost."
    );
}

/// A target that is a permanent or a player: Lightning Bolt's "Lightning Bolt deals 3 damage
/// to any target."
#[test]
fn any_target_is_said_without_the_word_target() {
    static LIGHTNING_BOLT: CardDef = one_face(
        "Lightning Bolt",
        &[Face::EMPTY
            .types(Types::INSTANT)
            .abilities(&[Ability::spell()
                .targeting(&[TargetSpec::any_target()])
                .effect(&deal_damage_to_any_target(3))])],
    );
    assert_eq!(
        render(&LIGHTNING_BOLT),
        "Lightning Bolt deals 3 damage to any target."
    );
}

/// A delayed ability that acts on the object it carries says its trigger last: Flickerwisp's
/// "When this creature enters, exile another target permanent. Return that card to the
/// battlefield under its owner's control at the beginning of the next end step."
#[test]
fn a_delayed_ability_on_a_carried_object_says_its_trigger_last() {
    static FLICKERWISP: CardDef = one_face(
        "Flickerwisp",
        &[Face::EMPTY.types(Types::CREATURE).abilities(&[
            Ability::triggered(&Trigger::ENTERS)
                .targeting(&[TargetSpec::permanent(Filter::Any).other()])
                .effect(&sequence(&[exile(target()), delay_with(1, those())])),
            Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
                .effect(&return_to_battlefield(carried_object())),
        ])],
    );
    assert_eq!(
        render(&FLICKERWISP),
        "When this creature enters, exile another target permanent. Return that card to the \
         battlefield under its owner's control at the beginning of the next end step."
    );
}

/// A creature that can't block and returns itself from its owner's graveyard, as
/// Bloodghast's "This creature can't block." and "Whenever a land you control enters, you
/// may return this card from your graveyard to the battlefield." say (its ability word
/// aside).
#[test]
fn a_card_in_a_graveyard_calls_itself_this_card_and_a_creature_can_be_unable_to_block() {
    static CANT_BLOCK: RuleChange = RuleChange {
        players: Players::You,
        what: Governed::Block(Objects::This),
        when: When::Never,
    };
    static RETURN: Effect = return_to_battlefield(Objects::This);
    static GHAST: CardDef = one_face(
        "Test Ghast",
        &[Face::EMPTY.types(Types::CREATURE).abilities(&[
            Ability::rule_change(&CANT_BLOCK),
            Ability::triggered(&Trigger::of(
                TriggerEvent::Enters,
                Subject::A(Filter::All(&[
                    Filter::Types(Types::LAND),
                    Filter::Whose(&Players::You),
                ])),
            ))
            .in_graveyard()
            .effect(&may(you(), &RETURN)),
        ])],
    );
    assert_eq!(
        render(&GHAST),
        "This creature can't block.\nWhenever a land you control enters, you may return \
         this card from your graveyard to the battlefield."
    );
}

/// A counter total across a set of permanents, as in Chronicler of Heroes' "When this creature
/// enters, if you control a creature with a +1/+1 counter on it, draw a card." It must not
/// read as a counter on every creature, while a single permanent still says "on".
#[test]
fn a_counter_total_across_permanents_is_said_among_them() {
    static AT_LEAST_ONE: Condition = Condition::Compare(
        Amount::Counters(permanents(creature_you_control()), CounterKind::PlusOne),
        Comparison::AtLeast,
        Amount::Fixed(1),
    );
    static ON_THIS: Condition = Condition::Compare(
        Amount::Counters(this(), CounterKind::PlusOne),
        Comparison::AtLeast,
        Amount::Fixed(1),
    );
    static DRAW: Effect = draw(you(), 1);
    static AMONG: Effect = if_then(AT_LEAST_ONE, &DRAW);
    static ON: Effect = if_then(ON_THIS, &DRAW);
    let among = render_ability(&Ability::spell().effect(&AMONG));
    assert!(
        among.contains("there is a +1/+1 counter among creatures you control"),
        "{among}"
    );
    let on = render_ability(&Ability::spell().effect(&ON));
    assert!(on.contains("counter on "), "{on}");
}

/// A mana ability's product of several terms ([`Ability::mana_of`]), as Oracle text writes
/// each: fixed symbols run together, a counted term says what it counts, and units chosen
/// apart list every choice.
#[test]
fn a_product_of_terms_renders_as_oracle_text() {
    use crate::ManaColor::{Black, Blue, White};
    use crate::Term;
    use crate::effect::build::{permanents, yours};
    use crate::filter::Filter;
    use crate::mana::mana;
    static SWAMPS: Objects = permanents(Filter::All(&[
        Filter::Subtype(crate::Subtype::Swamp),
        yours(),
    ]));
    static SIGNET: [Term; 2] = [Term::one(White), Term::one(Blue)];
    static COFFERS: [Term; 1] = [Term::counted(Black, Amount::Count(&SWAMPS))];
    static GATE: [Term; 1] = [Term::each_of(0b11, 2)];
    let signet = Ability::mana_of(Cost::of(mana("{1}"), &[CostPart::TAP]), &SIGNET);
    assert_eq!(render_ability(&signet), "{1}, {T}: Add {W}{U}.");
    let coffers = Ability::mana_of(Cost::of(mana("{2}"), &[CostPart::TAP]), &COFFERS);
    assert_eq!(
        render_ability(&coffers),
        "{2}, {T}: Add {B} for each Swamp you control."
    );
    let gate = Ability::mana_of(Cost::of(mana("{W/U}"), &[CostPart::TAP]), &GATE);
    assert_eq!(
        render_ability(&gate),
        "{W/U}, {T}: Add {W}{W}, {W}{U}, or {U}{U}."
    );
}
