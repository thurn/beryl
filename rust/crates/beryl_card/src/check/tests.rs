use super::*;
use crate::cost::CastingKind;
use crate::effect::build::{
    TARGET, TARGET_PLAYER, THAT_PLAYER, YOU, add_mana, counter_and_exile, deal_damage_to, delay,
    destroy, discard, draw, explore, fateseal, for_each_player, get_emblem, if_then, may_cast_free,
    put_back, search, shuffle_into, unless, unless_pays, untap_up_to,
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
    const NO_TARGET: &[Ability] = &[Ability::spell().effect(&destroy(TARGET))];
    const A_PLAYER_DESTROYED: &[Ability] = &[Ability::spell()
        .targeting(&[A_PLAYER])
        .effect(&destroy(TARGET))];
    const A_CREATURE_DRAWS: &[Ability] = &[Ability::spell()
        .targeting(&[A_CREATURE])
        .effect(&draw(TARGET_PLAYER, 1))];
    const WELL_FORMED: &[Ability] =
        &[Ability::spell()
            .targeting(&[A_CREATURE, A_PLAYER])
            .effect(&Effect::Sequence(&[
                destroy(TARGET),
                draw(Players::Target(SpecIx::new(1)), 1),
            ]))];
    assert_eq!(checked(NO_TARGET), Err(Malformed::Target));
    assert_eq!(checked(A_PLAYER_DESTROYED), Err(Malformed::Target));
    assert_eq!(checked(A_CREATURE_DRAWS), Err(Malformed::Target));
    assert_eq!(checked(WELL_FORMED), Ok(()));
}

#[test]
fn that_player_outside_a_for_each_player_is_refused() {
    const OUTSIDE: &[Ability] = &[Ability::spell().effect(&draw(THAT_PLAYER, 1))];
    const INSIDE: &[Ability] =
        &[Ability::spell().effect(&for_each_player(Players::EachPlayer, &draw(THAT_PLAYER, 1)))];
    assert_eq!(checked(OUTSIDE), Err(Malformed::ThatPlayer));
    assert_eq!(checked(INSIDE), Ok(()));
}

#[test]
fn a_cost_the_face_cannot_be_cast_with_is_refused() {
    const KICKED: &[Ability] = &[Ability::spell().effect(&if_then(
        Condition::Paid(CastingKind::Kicker),
        &draw(YOU, 1),
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
        player: YOU,
        of: Objects::This,
        count: Amount::Fixed(1),
        except: CopyException::PowerToughness(2, 2),
    })];
    assert_eq!(checked(COPY), Err(Malformed::Copy));
}

#[test]
fn mana_added_outside_a_mana_ability_is_refused() {
    const RITUAL: &[Ability] =
        &[Ability::spell().effect(&add_mana(YOU, ManaSet::one(ManaColor::Black)))];
    const LAND: &[Ability] = &[Ability::tap_for_mana(ManaSet::one(ManaColor::Black))];
    assert_eq!(checked(RITUAL), Err(Malformed::AddMana));
    assert_eq!(checked(LAND), Ok(()));
}

#[test]
fn effect_data_declares_nothing_of_its_targets() {
    const DECLARED: &[Ability] = &[Ability::spell()
        .targeting(&[A_CREATURE.removal()])
        .effect(&destroy(TARGET))];
    assert_eq!(checked(DECLARED), Err(Malformed::Declared));
}

#[test]
fn a_countered_spell_put_anywhere_but_exile_is_refused() {
    const A_SPELL: TargetSpec = TargetSpec::one(TargetKind::Spell, Filter::Any);
    const REMAND: &[Ability] =
        &[Ability::spell()
            .targeting(&[A_SPELL])
            .effect(&Effect::CounterInto {
                spells: TARGET,
                zone: Zone::Hand,
            })];
    const DISSIPATE: &[Ability] = &[Ability::spell()
        .targeting(&[A_SPELL])
        .effect(&counter_and_exile(TARGET))];
    assert_eq!(checked(REMAND), Err(Malformed::Countered));
    assert_eq!(checked(DISSIPATE), Ok(()));
}

#[test]
fn a_delay_of_no_delayed_trigger_of_the_face_is_refused() {
    const AT_THE_NEXT_END_STEP: Ability =
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
            .effect(&draw(YOU, 1));
    const PAST_THE_END: &[Ability] = &[Ability::spell().effect(&delay(1))];
    const ITSELF: &[Ability] = &[Ability::spell().effect(&delay(0)), AT_THE_NEXT_END_STEP];
    const PRINTED: &[Ability] = &[
        Ability::spell().effect(&delay(1)),
        Ability::triggered(&Trigger::ENTERS).effect(&draw(YOU, 1)),
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
        &draw(YOU, 1),
        YOU,
        Cost::of_parts(&[CostPart::sacrifice(Filter::Any)]),
    ))];
    const MANA_AND_A_CARD: &[Ability] = &[Ability::spell().effect(&unless(
        &draw(YOU, 1),
        YOU,
        Cost::of(mana("{1}"), &[CostPart::discard(Filter::Any)]),
    ))];
    const MANA: &[Ability] =
        &[Ability::spell().effect(&unless_pays(&draw(YOU, 1), YOU, mana("{1}")))];
    const A_CARD: &[Ability] = &[Ability::spell().effect(&unless(
        &discard(YOU, 2),
        YOU,
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
        player: YOU,
        library: Players::EachOpponent,
        count: Amount::Fixed(2),
    })];
    const ONE: &[Ability] = &[Ability::spell().effect(&fateseal(YOU, Players::EachOpponent))];
    assert_eq!(checked(TWO), Err(Malformed::Fateseal));
    assert_eq!(checked(ONE), Ok(()));
}

/// The operands of the entries a card port added are walked as any other's.
#[test]
fn the_operands_of_every_entry_are_checked() {
    const DAMAGE: &[Ability] = &[Ability::spell().effect(&deal_damage_to(TARGET_PLAYER, 3))];
    const EXPLORE: &[Ability] = &[Ability::spell().effect(&explore(TARGET))];
    const CAST: &[Ability] = &[Ability::spell().effect(&may_cast_free(YOU, TARGET))];
    const MOST: &[Ability] = &[Ability::spell().effect(&Effect::Draw {
        player: YOU,
        count: Amount::Most(TARGET_PLAYER, &Amount::Life(YOU)),
    })];
    const A_TOKEN: &[Ability] = &[Ability::spell().effect(&if_then(
        Condition::Not(&Condition::Token(TARGET)),
        &draw(YOU, 1),
    ))];
    const UNTAP: &[Ability] = &[Ability::spell().effect(&untap_up_to(YOU, 2, TARGET))];
    const CARRYING: &[Ability] = &[
        Ability::spell().effect(&Effect::Delay {
            ability: crate::index::AbilityIx::new(1),
            carrying: Amount::ManaValue(TARGET),
        }),
        Ability::triggered(&Trigger::main_phase(YOU).next()).effect(&draw(YOU, 1)),
    ];
    const EMBLEM: CardDef = crate::definition::card("Test Emblem", &[Face::EMPTY]);
    const EMBLEM_FOR: &[Ability] = &[Ability::spell().effect(&get_emblem(TARGET_PLAYER, &EMBLEM))];
    const PUT_BACK: &[Ability] = &[Ability::spell().effect(&put_back(TARGET_PLAYER, 2))];
    const SHUFFLE: &[Ability] = &[Ability::spell().effect(&shuffle_into(YOU, TARGET))];
    const CARDS_IN: &[Ability] = &[Ability::spell().effect(&destroy(Objects::CardsIn {
        players: &TARGET_PLAYER,
        zones: &[Zone::Hand],
    }))];
    const DIG: &[Ability] = &[Ability::spell().effect(&Effect::Dig {
        player: YOU,
        look: Amount::ManaValue(TARGET),
        take: Taken::Put(1),
        rest: LibraryOrder::Random,
    })];
    const FATESEAL: &[Ability] = &[Ability::spell().effect(&fateseal(YOU, TARGET_PLAYER))];
    const SEARCH: &[Ability] =
        &[Ability::spell().effect(&search(TARGET_PLAYER, Filter::Any, Found::RevealToHand))];
    const THOSE_OF: &[Ability] = &[Ability::spell().effect(&Effect::Draw {
        player: YOU,
        count: Amount::ThoseOf(TARGET_PLAYER),
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
        CARRIED, THAT_CARD, add_mana_of_kind, choose, exile, look_at, permit_cast,
    };
    use crate::prompt::{Prompt, Purpose};
    // "That card", of an ability nothing triggered, and of one something did.
    const THAT: &[Ability] = &[Ability::spell().effect(&exile(THAT_CARD))];
    const DISCARDED: &[Ability] =
        &[Ability::triggered(&Trigger::discards(YOU)).effect(&exile(THAT_CARD))];
    // A number no effect settled for it.
    const UNCARRIED: &[Ability] = &[Ability::triggered(&Trigger::main_phase(YOU))
        .effect(&add_mana_of_kind(YOU, ManaColor::Colorless, CARRIED))];
    // A choice the interpreter cannot ask: two objects, or no object at all.
    const TWO: &[Ability] = &[Ability::spell().effect(&choose(
        YOU,
        Objects::ExiledWithThis,
        Prompt::new(Purpose::Choose).most(2),
    ))];
    const YES_OR_NO: &[Ability] = &[Ability::spell().effect(&choose(
        YOU,
        Objects::ExiledWithThis,
        Prompt::new(Purpose::May).most(1),
    ))];
    // The battlefield is no player's own.
    const SHARED: &[Ability] = &[Ability::spell().effect(&look_at(
        YOU,
        Objects::InZoneOf {
            zone: Zone::Battlefield,
            players: &YOU,
            filter: Filter::Any,
        },
    ))];
    // A permission lasts this turn, or while the card stays where it is.
    const A_ROUND: &[Ability] = &[Ability::spell()
        .targeting(&[TargetSpec::one(TargetKind::GraveyardCard, Filter::Any)])
        .effect(&permit_cast(
            YOU,
            TARGET,
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

/// In a card's `static` this panic is the build failure.
#[test]
#[should_panic(expected = "an effect adds mana outside a mana ability")]
fn a_card_is_checked_as_it_is_built() {
    const RITUAL: &[Ability] =
        &[Ability::spell().effect(&add_mana(YOU, ManaSet::one(ManaColor::Black)))];
    let face: &'static Face = Box::leak(Box::new(Face::EMPTY.abilities(RITUAL)));
    let _ = crate::card("Test Ritual", std::slice::from_ref(face));
}
