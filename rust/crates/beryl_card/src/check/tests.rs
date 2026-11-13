use super::*;
use crate::cost::CastingKind;
use crate::effect::build::{
    add_mana, counter_and_exile, deal_damage_to, delay, destroy, discard, draw, explore, fateseal,
    for_each_player, get_emblem, if_then, may_cast_free, put_back, search, shuffle_into, target,
    target_player, that_player, unless, unless_pays, untap_up_to, you,
};
use crate::effect::{Found, LibraryOrder, Taken};
use crate::ids::TurnStep;
use crate::mana::mana;
use crate::mana::{ManaColor, ManaSet};
use crate::target::TargetSpec;
use crate::trigger::Trigger;

const A_CREATURE: TargetSpec = TargetSpec::one(TargetKind::Permanent, Filter::Any);
const A_PLAYER: TargetSpec = TargetSpec::one(TargetKind::Player, Filter::Any);

/// What the check says of a one-faced card with these abilities.
fn checked(abilities: &'static [Ability]) -> Checked {
    let face: &'static Face = Box::leak(Box::new(Face::EMPTY.abilities(abilities)));
    CardDef {
        name: "Test",
        faces: std::slice::from_ref(face),
        tokens: &[],
    }
    .check()
}

#[test]
fn a_target_the_ability_does_not_have_is_refused() {
    const NO_TARGET: &[Ability] = &[Ability::spell().effect(&destroy(target()))];
    const A_PLAYER_DESTROYED: &[Ability] = &[Ability::spell()
        .targeting(&[A_PLAYER])
        .effect(&destroy(target()))];
    const A_CREATURE_DRAWS: &[Ability] = &[Ability::spell()
        .targeting(&[A_CREATURE])
        .effect(&draw(target_player(), 1))];
    const WELL_FORMED: &[Ability] =
        &[Ability::spell()
            .targeting(&[A_CREATURE, A_PLAYER])
            .effect(&Effect::Sequence(&[
                destroy(target()),
                draw(Players::Target(SpecIx::new(1)), 1),
            ]))];
    assert_eq!(checked(NO_TARGET), Err(Malformed::Target));
    assert_eq!(checked(A_PLAYER_DESTROYED), Err(Malformed::Target));
    assert_eq!(checked(A_CREATURE_DRAWS), Err(Malformed::Target));
    assert_eq!(checked(WELL_FORMED), Ok(()));
}

#[test]
fn that_player_outside_a_for_each_player_is_refused() {
    const OUTSIDE: &[Ability] = &[Ability::spell().effect(&draw(that_player(), 1))];
    const INSIDE: &[Ability] = &[Ability::spell().effect(&for_each_player(
        Players::EachPlayer,
        &draw(that_player(), 1),
    ))];
    assert_eq!(checked(OUTSIDE), Err(Malformed::ThatPlayer));
    assert_eq!(checked(INSIDE), Ok(()));
}

#[test]
fn a_cost_the_face_cannot_be_cast_with_is_refused() {
    const KICKED: &[Ability] = &[Ability::spell().effect(&if_then(
        Condition::Paid(CastingKind::Kicker),
        &draw(you(), 1),
    ))];
    // Offspring's own ability, on a face that does not list its cost.
    const OFFSPRING: &[Ability] = &[Ability::offspring()];
    assert_eq!(checked(KICKED), Err(Malformed::Paid));
    assert_eq!(checked(OFFSPRING), Err(Malformed::Paid));
}

#[test]
fn a_grant_the_face_does_not_list_is_refused() {
    const GRANTS: &[Ability] = &[Ability::static_ability(&LayerSpec {
        affected: Objects::AttachedTo,
        condition: None,
        modification: Modification::grant(0),
    })];
    assert_eq!(checked(GRANTS), Err(Malformed::Grant));
}

#[test]
fn a_copy_that_is_not_one_one_is_refused() {
    const COPY: &[Ability] = &[Ability::spell().effect(&Effect::CreateCopies {
        player: you(),
        of: Objects::This,
        count: Amount::Fixed(1),
        except: CopyException::PowerToughness(2, 2),
    })];
    assert_eq!(checked(COPY), Err(Malformed::Copy));
}

#[test]
fn mana_added_outside_a_mana_ability_is_refused() {
    const RITUAL: &[Ability] =
        &[Ability::spell().effect(&add_mana(you(), ManaSet::one(ManaColor::Black)))];
    const LAND: &[Ability] = &[Ability::tap_for_mana(ManaSet::one(ManaColor::Black))];
    assert_eq!(checked(RITUAL), Err(Malformed::AddMana));
    assert_eq!(checked(LAND), Ok(()));
}

#[test]
fn a_countered_spell_put_anywhere_but_exile_is_refused() {
    const A_SPELL: TargetSpec = TargetSpec::one(TargetKind::Spell, Filter::Any);
    const REMAND: &[Ability] =
        &[Ability::spell()
            .targeting(&[A_SPELL])
            .effect(&Effect::CounterInto {
                spells: target(),
                zone: Zone::Hand,
            })];
    const DISSIPATE: &[Ability] = &[Ability::spell()
        .targeting(&[A_SPELL])
        .effect(&counter_and_exile(target()))];
    assert_eq!(checked(REMAND), Err(Malformed::Countered));
    assert_eq!(checked(DISSIPATE), Ok(()));
}

#[test]
fn a_delay_of_no_delayed_trigger_of_the_face_is_refused() {
    const AT_THE_NEXT_END_STEP: Ability =
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
            .effect(&draw(you(), 1));
    const PAST_THE_END: &[Ability] = &[Ability::spell().effect(&delay(1))];
    const ITSELF: &[Ability] = &[Ability::spell().effect(&delay(0)), AT_THE_NEXT_END_STEP];
    const PRINTED: &[Ability] = &[
        Ability::spell().effect(&delay(1)),
        Ability::triggered(&Trigger::ENTERS).effect(&draw(you(), 1)),
    ];
    const WELL_FORMED: &[Ability] = &[Ability::spell().effect(&delay(1)), AT_THE_NEXT_END_STEP];
    for abilities in [PAST_THE_END, ITSELF, PRINTED] {
        assert_eq!(checked(abilities), Err(Malformed::Delayed));
    }
    assert_eq!(checked(WELL_FORMED), Ok(()));
}

#[test]
fn a_cost_the_engine_cannot_take_as_an_effect_resolves_is_refused() {
    const SACRIFICE: &[Ability] = &[Ability::spell().effect(&unless(
        &draw(you(), 1),
        you(),
        Cost::of_parts(&[CostPart::sacrifice(Filter::Any)]),
    ))];
    const MANA_AND_A_CARD: &[Ability] = &[Ability::spell().effect(&unless(
        &draw(you(), 1),
        you(),
        Cost::of(mana("{1}"), &[CostPart::discard(Filter::Any)]),
    ))];
    const MANA: &[Ability] =
        &[Ability::spell().effect(&unless_pays(&draw(you(), 1), you(), mana("{1}")))];
    const A_CARD: &[Ability] = &[Ability::spell().effect(&unless(
        &discard(you(), 2),
        you(),
        Cost::of_parts(&[CostPart::discard(Filter::Any)]),
    ))];
    assert_eq!(checked(SACRIFICE), Err(Malformed::Cost));
    assert_eq!(checked(MANA_AND_A_CARD), Err(Malformed::Cost));
    assert_eq!(checked(MANA), Ok(()));
    assert_eq!(checked(A_CARD), Ok(()));
}

#[test]
fn a_fateseal_of_more_than_one_card_is_refused() {
    const TWO: &[Ability] = &[Ability::spell().effect(&Effect::Fateseal {
        player: you(),
        library: Players::EachOpponent,
        count: Amount::Fixed(2),
    })];
    const ONE: &[Ability] = &[Ability::spell().effect(&fateseal(you(), Players::EachOpponent))];
    assert_eq!(checked(TWO), Err(Malformed::Fateseal));
    assert_eq!(checked(ONE), Ok(()));
}

/// The operands of the entries a card port added are walked as any other's.
#[test]
fn the_operands_of_every_entry_are_checked() {
    const DAMAGE: &[Ability] = &[Ability::spell().effect(&deal_damage_to(target_player(), 3))];
    const EXPLORE: &[Ability] = &[Ability::spell().effect(&explore(target()))];
    const CAST: &[Ability] = &[Ability::spell().effect(&may_cast_free(you(), target()))];
    const MOST: &[Ability] = &[Ability::spell().effect(&Effect::Draw {
        player: you(),
        count: Amount::Most(target_player(), &Amount::Life(you())),
    })];
    const A_TOKEN: &[Ability] = &[Ability::spell().effect(&if_then(
        Condition::Not(&Condition::Token(target())),
        &draw(you(), 1),
    ))];
    const UNTAP: &[Ability] = &[Ability::spell().effect(&untap_up_to(you(), 2, target()))];
    const CARRYING: &[Ability] = &[
        Ability::spell().effect(&Effect::Delay {
            ability: crate::index::AbilityIx::new(1),
            carrying: Amount::ManaValue(target()),
        }),
        Ability::triggered(&Trigger::main_phase(you()).next()).effect(&draw(you(), 1)),
    ];
    const EMBLEM: CardDef = crate::definition::card("Test Emblem", &[Face::EMPTY]);
    const EMBLEM_FOR: &[Ability] =
        &[Ability::spell().effect(&get_emblem(target_player(), &EMBLEM))];
    const PUT_BACK: &[Ability] = &[Ability::spell().effect(&put_back(target_player(), 2))];
    const SHUFFLE: &[Ability] = &[Ability::spell().effect(&shuffle_into(you(), target()))];
    const CARDS_IN: &[Ability] = &[Ability::spell().effect(&destroy(Objects::CardsIn {
        players: &target_player(),
        zones: &[Zone::Hand],
    }))];
    const DIG: &[Ability] = &[Ability::spell().effect(&Effect::Dig {
        player: you(),
        look: Amount::ManaValue(target()),
        take: Taken::Put(1),
        rest: LibraryOrder::Random,
    })];
    const FATESEAL: &[Ability] = &[Ability::spell().effect(&fateseal(you(), target_player()))];
    const SEARCH: &[Ability] =
        &[Ability::spell().effect(&search(target_player(), Filter::Any, Found::RevealToHand))];
    const THOSE_OF: &[Ability] = &[Ability::spell().effect(&Effect::Draw {
        player: you(),
        count: Amount::ThoseOf(target_player()),
    })];
    for abilities in [
        DAMAGE, EXPLORE, CAST, MOST, A_TOKEN, UNTAP, CARRYING, EMBLEM_FOR, PUT_BACK, SHUFFLE,
        CARDS_IN, DIG, FATESEAL, SEARCH, THOSE_OF,
    ] {
        assert_eq!(checked(abilities), Err(Malformed::Target));
    }
}

/// What this port's entries can say that no game could carry out.
#[test]
fn what_the_port_entries_cannot_mean_is_refused() {
    use crate::effect::CastTerms;
    use crate::effect::build::{
        add_mana_of_kind, carried, choose, exile, look_at, permit_cast, that_card,
    };
    use crate::prompt::{Prompt, Purpose};
    // "That card", of an ability nothing triggered, and of one something did.
    const THAT: &[Ability] = &[Ability::spell().effect(&exile(that_card()))];
    const DISCARDED: &[Ability] =
        &[Ability::triggered(&Trigger::discards(you())).effect(&exile(that_card()))];
    // A number no effect settled for it.
    const UNCARRIED: &[Ability] = &[Ability::triggered(&Trigger::main_phase(you()))
        .effect(&add_mana_of_kind(you(), ManaColor::Colorless, carried()))];
    // A choice the interpreter cannot ask: two objects, or no object at all.
    const TWO: &[Ability] = &[Ability::spell().effect(&choose(
        you(),
        Objects::ExiledWithThis,
        Prompt::new(Purpose::Choose).most(2),
    ))];
    const YES_OR_NO: &[Ability] = &[Ability::spell().effect(&choose(
        you(),
        Objects::ExiledWithThis,
        Prompt::new(Purpose::May).most(1),
    ))];
    // The battlefield is no player's own.
    const SHARED: &[Ability] = &[Ability::spell().effect(&look_at(
        you(),
        Objects::InZoneOf {
            zone: Zone::Battlefield,
            players: &you(),
            filter: Filter::Any,
        },
    ))];
    // A permission lasts this turn, or while the card stays where it is.
    const A_ROUND: &[Ability] = &[Ability::spell()
        .targeting(&[TargetSpec::one(TargetKind::GraveyardCard, Filter::Any)])
        .effect(&permit_cast(
            you(),
            target(),
            Duration::UntilYourNextTurn,
            CastTerms::PLAIN,
        ))];
    assert_eq!(checked(THAT), Err(Malformed::TriggeredOn));
    assert_eq!(checked(DISCARDED), Ok(()));
    assert_eq!(checked(UNCARRIED), Err(Malformed::Carried));
    assert_eq!(checked(TWO), Err(Malformed::Chosen));
    assert_eq!(checked(YES_OR_NO), Err(Malformed::Chosen));
    assert_eq!(checked(SHARED), Err(Malformed::ZoneOf));
    assert_eq!(checked(A_ROUND), Err(Malformed::Permitted));
}

/// A card offered as the effect finishes is the effect's last word, and an ability names one
/// rule change to the game.
#[test]
fn what_an_effect_as_a_whole_cannot_mean_is_refused() {
    use crate::continuous::{Governed, RuleChange, When};
    use crate::effect::build::{change_rule, sequence, those};
    static AS_INSTANT: RuleChange = RuleChange {
        players: Players::You,
        what: Governed::ActivateLoyaltyAbilities(Objects::This),
        when: When::AsInstant,
    };
    const CAST: Effect = may_cast_free(you(), those());
    const CAST_THEN_DRAW: &[Ability] =
        &[Ability::spell().effect(&sequence(&[CAST, draw(you(), 1)]))];
    const DRAW_THEN_CAST: &[Ability] =
        &[Ability::spell().effect(&sequence(&[draw(you(), 1), if_then(Condition::Did, &CAST)]))];
    const EACH_CASTS: &[Ability] =
        &[Ability::spell().effect(&for_each_player(Players::EachPlayer, &CAST))];
    const RULE: Effect = change_rule(&AS_INSTANT, Duration::EndOfTurn);
    const ONE_RULE: &[Ability] = &[Ability::spell().effect(&sequence(&[draw(you(), 1), RULE]))];
    const TWO_RULES: &[Ability] = &[Ability::spell().effect(&sequence(&[RULE, RULE]))];
    assert_eq!(checked(CAST_THEN_DRAW), Err(Malformed::Cast));
    assert_eq!(checked(DRAW_THEN_CAST), Ok(()));
    assert_eq!(checked(EACH_CASTS), Err(Malformed::Cast));
    assert_eq!(checked(ONE_RULE), Ok(()));
    assert_eq!(checked(TWO_RULES), Err(Malformed::Rules));
}

/// What an object was is asked only where the interpreter keeps it (CR 608.2h): a moved
/// target's controller and numbers, and the controllers of what the instruction before moved.
#[test]
fn what_a_moved_object_was_is_refused_where_it_is_not_kept() {
    use crate::effect::build::{
        controller_of, exile, gain_life, its_controller, lose_life, sacrifice, sequence, this,
        those,
    };
    const fn spell(effect: &'static Effect) -> [Ability; 1] {
        [Ability::spell().targeting(&[A_CREATURE]).effect(effect)]
    }
    const IS_A_CREATURE: Condition = Condition::Is(target(), Filter::Any);
    const DRAW_IF_IT_IS: Effect = if_then(IS_A_CREATURE, &draw(you(), 1));
    const ASKED_BEFORE: &[Ability] = &spell(&sequence(&[DRAW_IF_IT_IS, destroy(target())]));
    const ASKED_AFTER: &[Ability] = &spell(&sequence(&[destroy(target()), DRAW_IF_IT_IS]));
    // One way through the effect moved it.
    const ASKED_AFTER_A_BRANCH: &[Ability] = &spell(&sequence(&[
        if_then(Condition::Did, &exile(target())),
        DRAW_IF_IT_IS,
    ]));
    // The second player's turn at it comes after the first's moved it.
    const ASKED_BY_EACH: &[Ability] = &spell(&for_each_player(
        Players::EachPlayer,
        &sequence(&[DRAW_IF_IT_IS, destroy(target())]),
    ));
    const TOKEN_AFTER: &[Ability] = &spell(&sequence(&[
        destroy(target()),
        if_then(Condition::Token(target()), &draw(you(), 1)),
    ]));
    // The new object the move made is asked where it is (CR 400.7j).
    const THOSE_ASKED_AFTER: &[Ability] = &spell(&sequence(&[
        destroy(target()),
        if_then(Condition::Is(those(), Filter::Any), &draw(you(), 1)),
    ]));
    const KEPT_OF_A_TARGET: &[Ability] = &spell(&sequence(&[
        destroy(target()),
        lose_life(its_controller(), 1),
        lose_life(controller_of(those()), 1),
        Effect::GainLife {
            player: you(),
            amount: Amount::Power(target()),
        },
    ]));
    const SOURCE_CONTROLLER_AFTER: &[Ability] = &spell(&sequence(&[
        sacrifice(this()),
        gain_life(controller_of(this()), 1),
    ]));
    const SOURCE_POWER_AFTER: &[Ability] = &spell(&sequence(&[
        exile(this()),
        Effect::GainLife {
            player: you(),
            amount: Amount::Power(this()),
        },
    ]));
    // An Aura that has left enchants nothing.
    const ENCHANTED_AFTER: &[Ability] = &spell(&sequence(&[
        sacrifice(this()),
        gain_life(controller_of(Objects::AttachedTo), 1),
    ]));
    assert_eq!(checked(ASKED_BEFORE), Ok(()));
    assert_eq!(checked(THOSE_ASKED_AFTER), Ok(()));
    assert_eq!(checked(KEPT_OF_A_TARGET), Ok(()));
    for refused in [
        ASKED_AFTER,
        ASKED_AFTER_A_BRANCH,
        ASKED_BY_EACH,
        TOKEN_AFTER,
        SOURCE_CONTROLLER_AFTER,
        SOURCE_POWER_AFTER,
        ENCHANTED_AFTER,
    ] {
        assert_eq!(checked(refused), Err(Malformed::LastKnown));
    }
}

/// In a card's `static` this panic is the build failure.
#[test]
#[should_panic(expected = "an effect adds mana outside a mana ability")]
fn a_card_is_checked_as_it_is_built() {
    const RITUAL: &[Ability] =
        &[Ability::spell().effect(&add_mana(you(), ManaSet::one(ManaColor::Black)))];
    let face: &'static Face = Box::leak(Box::new(Face::EMPTY.abilities(RITUAL)));
    let _ = crate::card("Test Ritual", std::slice::from_ref(face));
}
