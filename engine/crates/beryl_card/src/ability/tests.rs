use super::*;
use crate::characteristics::Keywords;
use crate::continuous::Modification;

/// "As long as you do" is no static ability: nothing was done before it. In a card's
/// `static` the same assertion stops the build.
#[test]
#[should_panic(expected = "a static ability has no targets")]
fn a_static_ability_that_needs_a_resolution_is_refused() {
    let _ = Ability::static_ability(&LayerSpec {
        affected: Objects::This,
        condition: Some(Condition::Did),
        modifications: &[Modification::AddKeywords(Keywords::FLYING)],
    });
}

/// A kind holds only what it has: each of these is a build failure in a card's `static`.
#[test]
#[should_panic(expected = "only a spell, activated or triggered ability has targets")]
fn a_replacement_with_targets_is_refused() {
    let _ = Ability::replacing(&Replacement::ENTERS_TAPPED).targeting(&[]);
}

#[test]
#[should_panic(expected = "a mana ability's is what it")]
fn a_mana_ability_with_another_effect_is_refused() {
    let _ = Ability::tap_for_mana(ManaSet::one(crate::ManaColor::Blue)).effect(&Effect::NOTHING);
}

#[test]
fn a_mana_ability_adds_what_it_produces() {
    let produces = ManaSet::one(crate::ManaColor::Green);
    let ability = Ability::tap_for_mana(produces);
    assert!(matches!(
        ability.body(),
        Behavior::Data(Effect::AddMana { mana, .. }) if *mana == produces
    ));
}

#[test]
fn custom_gives_back_only_the_type_it_was_made_of() {
    static BODY: fn() -> u8 = || 7;
    let custom = Custom::new(&BODY, &Says::NOTHING);
    assert_eq!(custom.get::<fn() -> u8>().map(|body| body()), Some(7));
    assert!(custom.get::<u8>().is_none());
}
