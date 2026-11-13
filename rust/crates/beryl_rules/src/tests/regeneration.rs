//! Regeneration shields ([CR 701.19a]): which destructions they replace, what they do instead,
//! and when they end. Drudge Skeletons' scenarios play the same through a card; these cover
//! what no implemented card does to a shielded creature, a sacrifice and toughness 0.
//!
//! [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a

use beryl_card::Regeneration;

use super::events::{game, put, resolve_all, with_game};
use crate::actions::destroy::{destroy, destroy_all};
use crate::actions::sacrifice::sacrifice;
use crate::bitboard::Bitboard;
use crate::continuous;
use crate::ids::{Seat, Slot};
use crate::state::{Continuous, Duration, LayerChange, State, Zone};
use crate::{GameTable, Scratch};

/// A Test Bear (2/2) of the starting player's on the battlefield with one regeneration shield.
fn shielded_bear() -> (GameTable, State, Scratch, Seat, Slot) {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        let shield = Continuous::Regeneration(bear_ref);
        continuous::start(game, resolution, shield, Duration::EndOfTurn).unwrap();
    });
    assert!(s.is_shielded(bear_ref));
    (t, s, x, p, bear)
}

#[test]
fn a_shield_replaces_one_destruction_and_the_next_destroys() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy(game, bear_ref).unwrap();
    });
    // CR 701.19a: "instead remove all damage marked on it and its controller taps it."
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
    assert!(s.object(bear).is_tapped());
    assert!(!s.is_shielded(bear_ref));
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy(game, bear_ref).unwrap();
    });
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
}

#[test]
fn a_shield_saves_a_creature_with_lethal_damage_and_removes_the_damage() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    let _ = s.mark_damage(bear_ref, bear_ref, 2, false);
    // CR 704.5g destroys it, which the shield replaces; with its damage removed, the repeated
    // check finds nothing more.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
    assert_eq!(s.object(bear).damage(), 0);
    assert!(s.object(bear).is_tapped());
    assert!(!s.is_shielded(bear_ref));
}

#[test]
fn a_shield_does_not_save_a_creature_with_toughness_zero() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        let shrink = Continuous::Layer(bear_ref, LayerChange::ModifyPt(-2, -2));
        continuous::start(game, resolution, shrink, Duration::EndOfTurn).unwrap();
    });
    // CR 704.5f puts it into its owner's graveyard, which is no destruction.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
}

#[test]
fn a_shield_does_not_save_a_sacrificed_permanent() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        sacrifice(game, bear_ref).unwrap();
    });
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
}

#[test]
fn a_shield_does_not_apply_to_a_destruction_that_forbids_regeneration() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy_all(game, Bitboard::single(bear), Regeneration::Forbidden).unwrap();
    });
    // CR 701.19c: such effects "cause regeneration shields to not be applied."
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
    assert!(!s.object(bear).is_tapped());
}

#[test]
fn a_shield_ends_with_the_turn() {
    let (t, mut s, _, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    // CR 514.2: "all 'until end of turn' and 'this turn' effects end".
    s.expire(Duration::EndOfTurn);
    assert!(!s.is_shielded(bear_ref));
    s.check_invariants(&t).unwrap();
}
