//! Preparation from instructions, and timing for an instant prepare spell.

use beryl_card::definition::{Shape, card_with_shape};
use beryl_card::prelude::*;

use super::LAND;
use super::cast::{BEAR, Game, cast, index_of};
use crate::state::Zone;
use crate::{CardTable, Opt, Profile};

static PREPARATION: CardDef = card_with_shape(
    "Test Preparation",
    Shape::Prepared,
    &[
        Face::EMPTY
            .types(Types::CREATURE)
            .power_toughness(2, 2)
            .abilities(&[Ability::replacing(&Replacement::ENTERS_PREPARED)]),
        Face::EMPTY
            .named("Test Instant Inset")
            .cost(ManaCost::generic(1))
            .types(Types::INSTANT)
            .for_shape(Shape::Prepared, FaceIx::BACK)
            .abilities(&[Ability::spell().effect(&gain_life(you(), 1))]),
    ],
);

static PREPARE: CardDef = Face::EMPTY
    .named("Test Prepare")
    .types(Types::SORCERY)
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&prepare(target()))])
    .card();

static UNPREPARE: CardDef = Face::EMPTY
    .named("Test Unprepare")
    .types(Types::SORCERY)
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&unprepare(target()))])
    .card();

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &PREPARATION, &PREPARE, &UNPREPARE]);

fn game() -> Game {
    Game::with(
        &CARDS,
        &[
            (15, LAND.name),
            (1, BEAR.name),
            (1, PREPARATION.name),
            (3, PREPARE.name),
            (2, UNPREPARE.name),
        ],
        Profile::EXACT,
    )
}

#[test]
fn an_instant_prepare_spell_can_be_cast_on_the_opponents_turn() {
    let mut game = game();
    let permanent = game.give(game.me, PREPARATION.name, Zone::Hand);
    let land = game.give(game.me, LAND.name, Zone::Battlefield);
    let priority = game.current();
    assert!(index_of(&priority, cast(permanent)).is_some());
    assert!(
        index_of(
            &priority,
            Opt::Cast {
                slot: permanent,
                face: FaceIx::BACK,
                costs: Castings::NONE,
            }
        )
        .is_none()
    );
    game.pick(cast(permanent));
    game.pass_both();

    let copies = game.s.in_zone(Zone::Exile);
    let copy = super::fixture_slots(copies).next().unwrap();
    assert_eq!(copies.count(), 1);
    assert_eq!(game.s.name(&game.t, copy), "Test Instant Inset");

    game.main(game.them);
    assert!(!game.offers_cast(copy));
    game.choose(crate::Choice(0));
    assert!(game.offers_cast(copy));
    game.pick(Opt::Cast {
        slot: copy,
        face: FaceIx::BACK,
        costs: Castings::NONE,
    });
    assert!(game.s.object(land).is_tapped());
    assert!(!game.s.is_prepared(permanent));
    game.pass_both();
    assert_eq!(game.s.player(game.me).life(), 21);
    game.decide();
    assert_eq!(game.s.object(copy).zone(), Zone::Nowhere);
}

#[test]
fn preparing_and_unpreparing_only_changes_a_designation_once() {
    let mut game = game();
    let permanent = game.give(game.me, PREPARATION.name, Zone::Hand);
    let bear = game.give(game.me, BEAR.name, Zone::Battlefield);
    let prepare_normal = game.give(game.me, PREPARE.name, Zone::Hand);

    game.pick(cast(prepare_normal));
    game.pass_both();
    assert!(!game.s.is_prepared(bear));
    assert!(game.s.in_zone(Zone::Exile).is_empty());

    game.pick(cast(permanent));
    game.pass_both();
    let copy = super::fixture_slots(game.s.in_zone(Zone::Exile))
        .next()
        .unwrap();
    let first = game.s.object_ref(copy);

    let prepare_again = game.give(game.me, PREPARE.name, Zone::Hand);
    game.pick(cast(prepare_again));
    game.pick_object(permanent);
    game.pass_both();
    assert!(game.s.is_current(first));
    assert_eq!(game.s.in_zone(Zone::Exile).count(), 1);

    for _ in 0..2 {
        let instruction = game.give(game.me, UNPREPARE.name, Zone::Hand);
        game.pick(cast(instruction));
        game.pick_object(permanent);
        game.pass_both();
        assert!(!game.s.is_prepared(permanent));
        assert!(!game.s.is_current(first));
        assert!(game.s.in_zone(Zone::Exile).is_empty());
    }

    let prepare_after = game.give(game.me, PREPARE.name, Zone::Hand);
    game.pick(cast(prepare_after));
    game.pick_object(permanent);
    game.pass_both();
    assert!(game.s.is_prepared(permanent));
    assert_eq!(game.s.in_zone(Zone::Exile).count(), 1);
}

#[test]
fn becoming_a_copy_keeps_the_prepare_spell_without_the_designation() {
    let mut game = game();
    let original = game.give(game.me, PREPARATION.name, Zone::Hand);
    let bear = game.give(game.me, BEAR.name, Zone::Battlefield);
    game.pick(cast(original));
    game.pass_both();

    let preparation = game.s.copiable(original);
    let source = preparation.record(&game.t).0;
    let bear_ref = game.s.object_ref(bear);
    game.s.become_copy(source, bear_ref, preparation).unwrap();
    assert!(!game.s.is_prepared(bear));
    assert_eq!(game.s.in_zone(Zone::Exile).count(), 1);

    let instruction = game.give(game.me, PREPARE.name, Zone::Hand);
    game.pick(cast(instruction));
    game.pick_object(bear);
    game.pass_both();
    assert!(game.s.is_prepared(bear));
    assert_eq!(game.s.in_zone(Zone::Exile).count(), 2);
}
