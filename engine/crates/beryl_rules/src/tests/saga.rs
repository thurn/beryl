//! Lore counter changes that cannot be set up with the implemented card pool.

use beryl_card::prelude::{add_counters, gain_life, target, you};
use beryl_card::{Ability, CardDef, Face, Subtype, SubtypeWords, Subtypes, Types};
use beryl_kernel::format::Format;

use super::events::find;
use crate::card::TargetSpec;
use crate::filter::Filter;
use crate::game::Game;
use crate::ids::{Seat, Slot};
use crate::replacement::event::Sink;
use crate::state::{CounterKind, TurnStep, Zone};
use crate::{CardTable, GameTable, Profile, State};

static SAGA: CardDef = CardDef {
    name: "Test Saga",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Saga])),
        abilities: &[
            Ability::chapters::<1, 2>().effect(&gain_life(you(), 1)),
            Ability::chapter(3)
                .target(&TargetSpec::permanent(Filter::Any))
                .effect(&add_counters(target(), CounterKind::PlusOne, 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&SAGA, &super::LAND]);

fn setup(lore: u8) -> (GameTable, State, Slot) {
    let deck = [(20, super::LAND.name), (1, SAGA.name)];
    let table = GameTable::new(Format::CONSTRUCTED, &CARDS, [&deck, &deck]).unwrap();
    let mut state = State::new(&table, 3, Profile::EXACT).unwrap();
    state.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let saga = find(&state, &table, Seat::P0, SAGA.name);
    state.board().place(saga, Zone::Battlefield).unwrap();
    let object = state.current(state.object_ref(saga)).unwrap();
    if lore > 0 {
        let _ = state
            .add_counters(object, CounterKind::Lore, lore, false)
            .unwrap();
    }
    (table, state, saga)
}

#[test]
fn removing_lore_does_not_trigger_but_adding_it_back_does() {
    let (table, mut state, saga) = setup(2);
    let object = state.permanent_at(saga).unwrap();
    let mut sink = Sink::off();
    let mut game = Game::new(&mut state, &table, &mut sink);
    crate::zones::counters::remove(&mut game, object, CounterKind::Lore, 1).unwrap();
    assert!(game.state().triggers().is_empty());
    crate::zones::counters::add(&mut game, object, CounterKind::Lore, 1, true).unwrap();
    assert_eq!(game.state().triggers().len(), 1);
    assert_eq!(game.state().counters_of(saga, CounterKind::Lore), 2);
}

#[test]
fn two_lore_counters_cross_both_chapters_in_one_range() {
    let (table, mut state, saga) = setup(0);
    let object = state.permanent_at(saga).unwrap();
    let mut sink = Sink::off();
    let mut game = Game::new(&mut state, &table, &mut sink);
    crate::zones::counters::add(&mut game, object, CounterKind::Lore, 2, true).unwrap();
    assert_eq!(game.state().triggers().len(), 2);
    assert_eq!(game.state().counters_of(saga, CounterKind::Lore), 2);
}

#[test]
fn a_final_chapter_waiting_for_targets_prevents_sacrifice() {
    let (table, mut state, saga) = setup(1);
    let object = state.permanent_at(saga).unwrap();
    let mut sink = Sink::off();
    let mut game = Game::new(&mut state, &table, &mut sink);
    crate::zones::counters::add(&mut game, object, CounterKind::Lore, 2, true).unwrap();
    assert_eq!(state.triggers().len(), 2);
    assert_eq!(
        crate::stack::trigger::next_to_stack(&state)
            .unwrap()
            .1
            .as_slice()
            .len(),
        2
    );
    assert!(!crate::sba::check(&mut state, &table, &mut sink).unwrap());
    assert!(state.permanent_at(saga).is_some());
}
