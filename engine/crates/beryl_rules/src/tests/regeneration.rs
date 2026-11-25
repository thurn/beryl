//! Regeneration shields ([CR 701.19a]): which destructions they replace, what they do instead,
//! and when they end. Drudge Skeletons' scenarios play the same through a card; these cover
//! what no implemented card does to a shielded creature, a sacrifice and toughness 0.
//!
//! [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a

use beryl_card::{Objects, Regeneration};

use super::events::{BEAR, game, put, resolve_all, with_game};
use super::{LAND, begin};
use crate::actions::destroy::{destroy, destroy_all};
use crate::actions::sacrifice::sacrifice;
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, CardDef, Face, LayerSpec, Modification, Subtype, SubtypeWords, Subtypes, Types,
};
use crate::continuous;
use crate::ids::{Seat, Slot};
use crate::state::{Continuous, Duration, LayerChange, State, Zone};
use crate::{CardTable, GameTable, Profile, Scratch};
use beryl_kernel::format::Format;

/// An Equipment that gives the equipped creature +1/-1.
static EQUIPMENT: CardDef = CardDef {
    name: "Test Clamp",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Equipment])),
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::ModifyPt(1, -1)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A Test Bear (2/2) of the starting player's on the battlefield with one regeneration shield.
fn shielded_bear() -> (GameTable, State, Scratch, Seat, Slot) {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let bear_ref = s.object_ref(bear);
    let bear_permanent = s.permanent(bear_ref).unwrap();
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        let shield = Continuous::Regeneration(bear_permanent);
        continuous::start(game, resolution, shield, Duration::EndOfTurn).unwrap();
    });
    assert!(s.permanent(bear_ref).is_some_and(|p| s.is_shielded(p)));
    (t, s, x, p, bear)
}

#[test]
fn a_shield_replaces_one_destruction_and_the_next_destroys() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy(game, game.state().permanent(bear_ref).unwrap()).unwrap();
    });
    // CR 701.19a: "instead remove all damage marked on it and its controller taps it."
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
    assert!(s.object(bear).is_tapped());
    assert!(!s.permanent(bear_ref).is_some_and(|p| s.is_shielded(p)));
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy(game, game.state().permanent(bear_ref).unwrap()).unwrap();
    });
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
}

#[test]
fn a_shield_saves_a_creature_with_lethal_damage_and_removes_the_damage() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    let _ = s.mark_damage(bear_ref, s.permanent(bear_ref).unwrap(), 2, false);
    // CR 704.5g destroys it, which the shield replaces; with its damage removed, the repeated
    // check finds nothing more.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
    assert_eq!(s.object(bear).damage(), 0);
    assert!(s.object(bear).is_tapped());
    assert!(!s.permanent(bear_ref).is_some_and(|p| s.is_shielded(p)));
}

#[test]
fn regeneration_uses_toughness_before_equipment_detaches() {
    static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &EQUIPMENT]);
    let deck = &[(1, "Test Bear"), (1, "Test Clamp"), (18, "Test Land")];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let equipment = put(&t, &mut s, &mut x, p, "Test Clamp");
    let bear_ref = s.object_ref(bear);
    let bear_permanent = s.permanent(bear_ref).unwrap();
    let equipment_permanent = s.permanent_at(equipment).unwrap();
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        let shield = Continuous::Regeneration(bear_permanent);
        continuous::start(game, resolution, shield, Duration::EndOfTurn).unwrap();
        game.state.attach(equipment_permanent, bear_permanent);
        let equipment = game.state().permanents_in(Bitboard::single(equipment));
        continuous::start_on_set(
            game,
            resolution,
            equipment,
            &[
                LayerChange::AddTypes(Types::CREATURE),
                LayerChange::SetBasePt(2, 2),
            ],
            Duration::EndOfTurn,
        )
        .unwrap();
    });
    assert_eq!(s.derived(bear).toughness, 1);
    let _ = s.mark_damage(bear_ref, bear_permanent, 1, false);

    // The damage is lethal when the check begins. The Equipment becoming unattached and
    // regeneration replacing destruction are both results of that check (CR 704.3).
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(equipment).attached_to(), None);
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
    assert_eq!(s.derived(bear).toughness, 2);
    assert_eq!(s.object(bear).damage(), 0);
    assert!(s.object(bear).is_tapped());
    assert!(!s.permanent(bear_ref).is_some_and(|p| s.is_shielded(p)));
}

#[test]
fn a_shield_does_not_save_a_creature_with_toughness_zero() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    let bear_ref = s.object_ref(bear);
    let bear_permanent = s.permanent(bear_ref).unwrap();
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        let shrink = Continuous::Layer(bear_permanent, LayerChange::ModifyPt(-2, -2));
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
        sacrifice(game, game.state().permanent(bear_ref).unwrap()).unwrap();
    });
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
}

#[test]
fn a_shield_does_not_apply_to_a_destruction_that_forbids_regeneration() {
    let (t, mut s, mut x, _, bear) = shielded_bear();
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy_all(
            game,
            game.state().permanents_in(Bitboard::single(bear)),
            Regeneration::Forbidden,
        )
        .unwrap();
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
    assert!(!s.permanent(bear_ref).is_some_and(|p| s.is_shielded(p)));
    s.check_invariants(&t).unwrap();
}
