use beryl_card::{
    CastingKind, Comparison, Filter, Keywords, Modification, Players, SpecIx, Types, Zone,
};

use super::*;

const GAINS_FLYING: Modification = Modification::AddKeywords(Keywords::FLYING);

fn spec(affected: Objects, condition: Option<Condition>) -> LayerSpec {
    LayerSpec {
        affected,
        condition,
        modifications: &[GAINS_FLYING],
    }
}

const fn at_least(amount: Amount, n: u8) -> Condition {
    Condition::Compare(amount, Comparison::AtLeast, Amount::Fixed(n))
}

const TIME_COUNTERS: Amount = Amount::Counters(Objects::This, CounterKind::Time);
const HAS_A_TIME_COUNTER: Condition = at_least(TIME_COUNTERS, 1);
/// Impending's condition (CR 702.176a).
const IMPENDING: Condition =
    Condition::All(&[Condition::Paid(CastingKind::Impending), HAS_A_TIME_COUNTER]);
const CREATURES_YOU_CONTROL: Objects = Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
};

#[test]
fn the_layers_apply_an_ability_of_this_permanent_or_the_one_it_is_attached_to() {
    for affected in [Objects::This, Objects::AttachedTo] {
        assert_eq!(applied(&spec(affected, None)), Ok(()));
    }
    for condition in [
        Condition::Paid(CastingKind::Impending),
        HAS_A_TIME_COUNTER,
        Condition::Not(&HAS_A_TIME_COUNTER),
        IMPENDING,
        Condition::Any(&[HAS_A_TIME_COUNTER, IMPENDING]),
    ] {
        assert_eq!(
            applied(&spec(Objects::This, Some(condition))),
            Ok(()),
            "{condition:?}"
        );
    }
}

/// "Creatures you control get +1/+1", "all artifacts are blue": the layers find the
/// permanents a filter accepts as they apply. Cards in a hand have no layer results, and
/// "mana value X or less" has no X to read.
#[test]
fn a_static_ability_over_the_permanents_a_filter_accepts_is_applied() {
    assert_eq!(applied(&spec(CREATURES_YOU_CONTROL, None)), Ok(()));
    let untapped_cheap = Objects::All {
        zone: Zone::Battlefield,
        filter: Filter::All(&[Filter::Not(&Filter::Tapped), Filter::ManaValueAtMost(2)]),
    };
    assert_eq!(applied(&spec(untapped_cheap, None)), Ok(()));
    let in_hand = Objects::All {
        zone: Zone::Hand,
        filter: Filter::Any,
    };
    assert_eq!(applied(&spec(in_hand, None)), Err(Unapplied::Affected));
}

/// A static ability has no resolution, so nothing was done before it, nothing was
/// targeted and no X was announced.
#[test]
fn a_static_ability_that_needs_a_resolution_is_refused() {
    let target = Objects::Target(SpecIx::new(0));
    for condition in [
        Condition::Did,
        at_least(Amount::ThatMany, 1),
        at_least(Amount::X, 1),
        at_least(Amount::Counters(Objects::Those, CounterKind::Time), 1),
        at_least(Amount::Power(target), 4),
        Condition::Is(target, Filter::Any),
        Condition::Is(Objects::This, Filter::ManaValueAtMostX),
        Condition::Is(Objects::This, Filter::Whose(&Players::ThatPlayer)),
        Condition::All(&[Condition::Paid(CastingKind::Evoke), Condition::Did]),
        Condition::Not(&Condition::Did),
    ] {
        assert_eq!(
            applied(&spec(Objects::This, Some(condition))),
            Err(Unapplied::Resolving),
            "{condition:?}"
        );
    }
    for affected in [target, Objects::Those] {
        assert_eq!(applied(&spec(affected, None)), Err(Unapplied::Resolving));
    }
}

/// "As long as you control a creature", "as long as it's a creature": the layers read
/// them as the effect's layer applies.
#[test]
fn a_condition_on_what_the_layers_decide_is_read_as_they_apply() {
    for condition in [
        at_least(Amount::Count(&CREATURES_YOU_CONTROL), 1),
        Condition::Is(Objects::This, Filter::Types(Types::CREATURE)),
        Condition::Any(&[
            HAS_A_TIME_COUNTER,
            Condition::Is(Objects::This, Filter::Tapped),
        ]),
    ] {
        assert_eq!(
            applied(&spec(Objects::This, Some(condition))),
            Ok(()),
            "{condition:?}"
        );
        assert!(reads_results(&condition), "{condition:?}");
    }
    for condition in [HAS_A_TIME_COUNTER, IMPENDING] {
        assert!(!reads_results(&condition), "{condition:?}");
    }
}

/// "As long as its power is 4 or greater", "as long as enchanted creature has a +1/+1
/// counter": nothing the layers read.
#[test]
fn a_condition_the_layers_cannot_read_is_refused() {
    for condition in [
        at_least(Amount::Power(Objects::This), 4),
        at_least(
            Amount::Counters(Objects::AttachedTo, CounterKind::PlusOne),
            1,
        ),
        at_least(Amount::Times(2, &TIME_COUNTERS), 2),
        Condition::Is(Objects::AttachedTo, Filter::Types(Types::CREATURE)),
    ] {
        assert_eq!(
            applied(&spec(Objects::This, Some(condition))),
            Err(Unapplied::Condition),
            "{condition:?}"
        );
    }
}

/// "Its power is equal to the number of card types among cards in all graveyards and its
/// toughness is equal to that number plus 1": a characteristic-defining ability whose
/// amounts read the graveyards, which the face declares as an input. Said of another
/// permanent, or with a condition, it is no characteristic-defining ability.
#[test]
fn a_defined_power_and_toughness_may_read_the_graveyards() {
    const TYPES: Amount = Amount::CardTypesAmong(&Objects::All {
        zone: Zone::Graveyard,
        filter: Filter::Any,
    });
    const DEFINES: &[Modification] = &[Modification::SetPt(&TYPES, &Amount::Plus(1, &TYPES))];
    static SPEC: LayerSpec = LayerSpec {
        affected: Objects::This,
        condition: None,
        modifications: DEFINES,
    };
    static COUNTED: LayerSpec = LayerSpec {
        affected: Objects::This,
        condition: Some(HAS_A_TIME_COUNTER),
        modifications: &[GAINS_FLYING],
    };
    static READS_GRAVEYARDS: &[beryl_card::Ability] = &[beryl_card::Ability::static_ability(&SPEC)];
    static READS_COUNTERS: &[beryl_card::Ability] =
        &[beryl_card::Ability::static_ability(&COUNTED)];
    let defines = |affected, condition| LayerSpec {
        affected,
        condition,
        modifications: DEFINES,
    };
    assert_eq!(applied(&defines(Objects::This, None)), Ok(()));
    assert_eq!(
        applied(&defines(CREATURES_YOU_CONTROL, None)),
        Err(Unapplied::SetPower)
    );
    assert_eq!(
        applied(&defines(Objects::This, Some(HAS_A_TIME_COUNTER))),
        Err(Unapplied::SetPower)
    );
    let face = |abilities| Face {
        abilities,
        ..Face::EMPTY
    };
    assert_eq!(Inputs::of(&face(READS_GRAVEYARDS)), Inputs::GRAVEYARDS);
    assert_eq!(Inputs::of(&face(READS_COUNTERS)), Inputs::COUNTERS);
    assert_eq!(Inputs::of(&face(&[])), Inputs::NONE);
}

#[test]
fn a_condition_reads_the_counters_it_compares() {
    assert!(reads_counters(&IMPENDING, CounterKind::Time));
    assert!(!reads_counters(&IMPENDING, CounterKind::Loyalty));
    assert!(reads_counters(
        &Condition::Not(&HAS_A_TIME_COUNTER),
        CounterKind::Time
    ));
    assert!(!reads_counters(
        &Condition::Paid(CastingKind::Impending),
        CounterKind::Time
    ));
}

#[test]
fn a_granted_ability_cannot_define_power_and_toughness() {
    static SPEC: LayerSpec = LayerSpec {
        affected: Objects::This,
        condition: None,
        modifications: &[Modification::SetPt(&Amount::Fixed(1), &Amount::Fixed(2))],
    };
    static FACES: &[Face] = &[Face::EMPTY.grants(&[beryl_card::Ability::static_ability(&SPEC)])];
    let card = CardDef {
        name: "Test",
        shape: beryl_card::definition::Shape::Single,
        faces: FACES,
        tokens: &[],
    };
    assert_eq!(check(&card), Err(Unapplied::GrantedAmount));
}

const NO_ICE: Condition = Condition::Compare(
    Amount::Counters(Objects::This, CounterKind::Ice),
    Comparison::Equal,
    Amount::Fixed(0),
);

/// "When you control no Islands" (Dandân) and "When this has no ice counters on it" (Dark
/// Depths) compare amounts whose changes mark the layer results stale; "during your turn"
/// changes with no recompute.
#[test]
fn a_state_trigger_comparing_what_the_layers_watch_is_watched() {
    static NO_ISLANDS: Condition = Condition::Compare(
        Amount::Count(&Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[
                Filter::Subtype(beryl_card::Subtype::Island),
                Filter::Whose(&Players::You),
            ]),
        }),
        Comparison::Equal,
        Amount::Fixed(0),
    );
    static NOT_NO_ICE: Condition = Condition::Not(&NO_ICE);
    static NO_ICE_ON_YOUR_TURN: Condition =
        Condition::All(&[NO_ICE, Condition::TurnOf(Players::You)]);
    assert!(watched(&NO_ISLANDS));
    assert!(watched(&NO_ICE));
    assert!(watched(&NOT_NO_ICE));
    assert!(!watched(&Condition::TurnOf(Players::You)));
    assert!(!watched(&NO_ICE_ON_YOUR_TURN));
}

/// A state trigger that compares counters makes a change in them mark the layer results stale.
#[test]
fn a_state_trigger_that_compares_counters_reads_them() {
    static TRIGGER: beryl_card::Trigger = beryl_card::Trigger::state(&NO_ICE);
    static ABILITIES: &[beryl_card::Ability] = &[beryl_card::Ability::triggered(&TRIGGER)];
    let face = Face {
        abilities: ABILITIES,
        ..Face::EMPTY
    };
    assert_eq!(Inputs::of(&face), Inputs::COUNTERS);
    assert!(state_condition(&ABILITIES[0]).is_some_and(watched));
}
