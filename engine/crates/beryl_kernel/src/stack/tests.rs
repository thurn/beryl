//! Tests of the stack's records.

use super::*;

/// A target comes out as what it went in as, and never as the other.
#[test]
fn a_target_is_an_object_or_a_player() {
    let spec = SpecIx::new(1);
    let object = ObjectRef {
        slot: crate::ids::Slot::new(1),
        incarnation: 1,
    };
    let chosen: Target = Target::object(spec, object);
    assert_eq!((chosen.spec(), chosen.as_object()), (spec, Some(object)));
    assert_eq!(chosen.as_player(), None);
    let chosen: Target = Target::player(spec, Seat::P1);
    assert_eq!((chosen.spec(), chosen.as_player()), (spec, Some(Seat::P1)));
    assert_eq!((chosen.as_object(), chosen.as_ability()), (None, None));
    let chosen: Target = Target::ability(spec, 1);
    assert_eq!(
        (chosen.spec(), chosen.as_ability()),
        (spec, Some(AbilityTarget::At(1)))
    );
    assert_eq!((chosen.as_object(), chosen.as_player()), (None, None));
}

/// When an entry leaves from below the top, a target that named it is gone, one that
/// named a place above it moves down one, and one below it or not an ability stays.
#[test]
fn a_removal_below_an_ability_target_repairs_it() {
    let spec = SpecIx::new(0);
    let at = |place| Target::<Small>::ability(spec, place);
    assert_eq!(
        at(2).after_removal(2).as_ability(),
        Some(AbilityTarget::Gone)
    );
    assert_eq!(at(3).after_removal(1), at(2));
    assert_eq!(at(1).after_removal(2), at(1));
    let gone = at(2).after_removal(2);
    assert_eq!(gone.after_removal(0), gone);
    let player = Target::<Small>::player(spec, Seat::P0);
    assert_eq!(player.after_removal(0), player);
}

/// Targets are placed in the order they were chosen, and a set of places holds what was
/// put in it.
#[test]
fn places_count_the_targets_in_the_order_chosen() {
    let targets: [Target; 3] = [
        Target::player(SpecIx::new(0), Seat::P0),
        Target::player(SpecIx::new(0), Seat::P1),
        Target::player(SpecIx::new(1), Seat::P0),
    ];
    let places: Vec<TargetPlace> = placed(&targets).map(|(place, _)| place).collect();
    assert_eq!(
        places,
        [TargetPlace::FIRST, TargetPlace::new(1), TargetPlace::new(2)]
    );
    let mut set = TargetPlaces::NONE;
    assert!(set.is_empty() && size_of::<TargetPlaces>() == 1);
    set.insert(places[2]);
    assert!(set.contains(places[2]) && !set.contains(places[0]) && !set.is_empty());
}
