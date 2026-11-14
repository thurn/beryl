//! Abilities that work outside the battlefield (CR 113.6) and state triggers (CR 603.8), each
//! shown with a test card no printed card of the pool is yet.

use beryl_card::effect::build::{gain_life, you};

use super::begin;
use super::events::{find, put, resolve_all};
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, Amount, CardDef, Castings, Condition, Face, Objects, Players, Replacement, Subject,
    Trigger, TriggerEvent, Types,
};
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::filter::Filter;
use crate::fixed::Fixed;
use crate::ids::{Seat, Slot};
use crate::layout::{Small, index};
use crate::state::{StackItem, Zone, ZoneKey};
use crate::table::TableError;
use crate::zone::Place;
use crate::{CardTable, Decklist, GameTable, Profile, Scratch, State};
use beryl_card::Comparison;
use beryl_kernel::listener::Unmatched;
use beryl_kernel::two_player::TwoPlayerSeat as _;

static LAND: CardDef = CardDef {
    name: "Test Land",
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Whenever you draw a card, if this card is in your graveyard, you gain 1 life."
static GHOST: CardDef = CardDef {
    name: "Test Ghost",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::draws(Players::You))
            .in_graveyard()
            .effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Whenever a creature dies, if this card is in your graveyard, you gain 1 life."
static MOURNER: CardDef = CardDef {
    name: "Test Graveyard Mourner",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::of(
            TriggerEvent::Dies,
            Subject::A(Filter::Types(Types::CREATURE)),
        ))
        .in_graveyard()
        .effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "When you cast this spell, you gain 1 life."
static HERALD: CardDef = CardDef {
    name: "Test Herald",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[
            Ability::triggered(&Trigger::of(TriggerEvent::Casts, Subject::This))
                .on_the_stack()
                .effect(&gain_life(you(), 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "When you have no cards in hand, you gain 1 life."
static LONER: CardDef = CardDef {
    name: "Test Loner",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::triggered(&Trigger::state(&Condition::Compare(
            Amount::Count(&Objects::All {
                zone: Zone::Hand,
                filter: Filter::Whose(&Players::You),
            }),
            Comparison::Equal,
            Amount::Fixed(0),
        )))
        .effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &GHOST, &MOURNER, &HERALD, &LONER]);

const DECK: &Decklist = &[
    (4, "Test Bear"),
    (2, "Test Ghost"),
    (1, "Test Graveyard Mourner"),
    (2, "Test Herald"),
    (1, "Test Loner"),
    (30, "Test Land"),
];

/// A game in the starting player's first main phase.
fn game() -> (GameTable, State, Seat, Scratch) {
    let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    (t, s, p, Scratch::new())
}

/// Moves the object in `slot` to `to` with no event, as a test arranges a position.
fn place(s: &mut State, slot: Slot, to: Zone) {
    s.move_object(&mut Sink::off(), slot, to, Place::Top)
        .unwrap();
}

/// `player` drawing one card, as the event of it.
fn draw(t: &GameTable, s: &mut State, x: &mut Scratch, player: Seat) {
    event::settle(s, t, &mut x.events, Event::Draw { player, cards: 1 }).unwrap();
}

#[test]
fn an_ability_that_works_in_a_graveyard_triggers_only_there() {
    let (t, mut s, p, mut x) = game();
    let ghost = put(&t, &mut s, &mut x, p, "Test Ghost");
    draw(&t, &mut s, &mut x, p);
    assert!(s.triggers().is_empty(), "not on the battlefield");
    place(&mut s, ghost, Zone::Graveyard);
    draw(&t, &mut s, &mut x, p.opponent());
    assert!(s.triggers().is_empty(), "not its owner's draw");
    draw(&t, &mut s, &mut x, p);
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].of.source, Some(s.object_ref(ghost)));
    assert_eq!(s.triggers()[0].of.controller, p, "its owner controls it");
    let life = s.player(p).life();
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), life + 1);
    place(&mut s, ghost, Zone::Exile);
    draw(&t, &mut s, &mut x, p);
    assert!(s.triggers().is_empty(), "nor in exile");
}

/// An ability in a graveyard looks back at a creature dying as one on the battlefield does
/// (CR 603.10a).
#[test]
fn an_ability_in_a_graveyard_sees_a_creature_die() {
    let (t, mut s, p, mut x) = game();
    let mourner = find(&s, &t, p, "Test Graveyard Mourner");
    place(&mut s, mourner, Zone::Graveyard);
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let dead = s.object_ref(bear);
    event::move_batch(
        &mut s,
        &t,
        &mut x.events,
        Bitboard::single(bear),
        Zone::Graveyard,
        Reason::Effect,
        Entry::default(),
    )
    .unwrap();
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].info.object, Some(dead));
    assert!(s.last_known(dead).is_some(), "the bear as it last existed");
}

#[test]
fn a_spell_triggers_on_its_own_cast_from_the_stack() {
    let (t, mut s, p, mut x) = game();
    let cast = |s: &State, spell: Slot| Event::Cast {
        spell: s.object_ref(spell),
        controller: p,
        targets: Fixed::default(),
        x: 0,
        costs: Castings::default(),
        flashback: false,
    };
    // On the battlefield, another spell's cast is none of its own.
    put(&t, &mut s, &mut x, p, "Test Herald");
    let bear = find(&s, &t, p, "Test Bear");
    place(&mut s, bear, Zone::Stack);
    let e = cast(&s, bear);
    event::settle(&mut s, &t, &mut x.events, e).unwrap();
    assert!(s.triggers().is_empty());
    let herald = find(&s, &t, p, "Test Herald");
    place(&mut s, herald, Zone::Stack);
    let e = cast(&s, herald);
    event::settle(&mut s, &t, &mut x.events, e).unwrap();
    assert_eq!(s.triggers().len(), 1, "the spell's own, once");
    assert_eq!(s.triggers()[0].of.source, Some(s.object_ref(herald)));
}

/// Empties `player`'s hand into their graveyard.
fn empty_hand(s: &mut State, player: Seat) {
    for slot in s.in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Hand))) {
        place(s, slot, Zone::Graveyard);
    }
}

#[test]
fn a_state_trigger_does_not_trigger_again_while_waiting_or_on_the_stack() {
    let (t, mut s, p, mut x) = game();
    put(&t, &mut s, &mut x, p, "Test Loner");
    crate::sba::check(&mut s, &t, &mut x.events).unwrap();
    assert!(s.triggers().is_empty(), "a hand with cards");
    empty_hand(&mut s, p);
    for _ in 0..2 {
        crate::sba::check(&mut s, &t, &mut x.events).unwrap();
        assert_eq!(s.triggers().len(), 1, "once while it waits");
    }
    crate::trigger::put_on_stack(&mut s, &t, &mut x.events, index::<Small>(0)).unwrap();
    crate::sba::check(&mut s, &t, &mut x.events).unwrap();
    assert!(s.triggers().is_empty(), "nor while it is on the stack");
    assert!(matches!(
        s.stack().last().map(|e| &e.item),
        Some(StackItem::Triggered { .. })
    ));
    // It resolves whatever the hand holds then: a state trigger has no intervening "if".
    let card = s.library(p)[0];
    place(&mut s, card, Zone::Hand);
    let life = s.player(p).life();
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), life + 1);
    assert!(s.triggers().is_empty(), "the hand holds a card");
    empty_hand(&mut s, p);
    crate::sba::check(&mut s, &t, &mut x.events).unwrap();
    assert_eq!(
        s.triggers().len(),
        1,
        "it triggers again once it has left the stack"
    );
}

/// Abilities that say they work where the engine looks for none of their kind.
static RECLUSE: CardDef = CardDef {
    name: "Test Recluse",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::replacing(Replacement::EntersTapped).in_graveyard()],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static WANDERER: CardDef = CardDef {
    name: "Test Wanderer",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::triggered(&Trigger::draws(Players::You))
            .everywhere()
            .effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static REFUSED: CardTable = CardTable::new(&[&LAND, &RECLUSE, &WANDERER]);

#[test]
fn the_table_refuses_an_ability_that_works_where_the_engine_does_not_look() {
    let table = |name: &'static str| {
        let deck: &Decklist = &[(1, name), (39, "Test Land")];
        GameTable::new(&REFUSED, [deck, deck]).map(|_| ())
    };
    assert_eq!(
        table("Test Recluse"),
        Err(TableError::Zone {
            card: "Test Recluse"
        })
    );
    // A hand or a library, where a card may yet be dealt anew.
    assert_eq!(
        table("Test Wanderer"),
        Err(TableError::Trigger {
            card: "Test Wanderer",
            unmatched: Unmatched::Zone
        })
    );
}
