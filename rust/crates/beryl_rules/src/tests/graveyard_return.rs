//! A card that returns itself from its owner's graveyard to the battlefield
//! ([`beryl_card::Effect::ReturnToBattlefield`] of the ability's source, from an ability that
//! works only in a graveyard, CR 113.6b), shown with a test card no printed card of the pool
//! is yet.

use beryl_card::effect::build::return_to_battlefield;

use super::begin;
use super::events::{find, put, resolve_all};
use crate::card::{
    Ability, CardDef, Face, Objects, Players, Subject, Trigger, TriggerEvent, Types,
};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::replacement::event::{self, Reason, Sink};
use crate::state::Zone;
use crate::zone::Place;
use crate::{CardTable, Decklist, GameTable, Profile, Scratch, State};
use beryl_kernel::format::Format;
use beryl_kernel::two_player::TwoPlayerSeat as _;

static LAND: CardDef = CardDef {
    name: "Test Land",
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Whenever a land you control enters, return this card from your graveyard to the
/// battlefield."
static REVENANT: CardDef = CardDef {
    name: "Test Revenant",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::of(
            TriggerEvent::Enters,
            Subject::A(Filter::All(&[
                Filter::Types(Types::LAND),
                Filter::Whose(&Players::You),
            ])),
        ))
        .in_graveyard()
        .effect(&return_to_battlefield(Objects::This))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &REVENANT]);

const DECK: &Decklist = &[(4, "Test Revenant"), (36, "Test Land")];

/// A game in the starting player's first main phase.
fn game() -> (GameTable, State, Seat, Scratch) {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).unwrap();
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

/// A land of `player`'s enters the battlefield, as the event of it.
fn land_enters(t: &GameTable, s: &mut State, x: &mut Scratch, player: Seat) {
    let land = find(s, t, player, "Test Land");
    event::move_one(s, t, &mut x.events, land, Zone::Battlefield, Reason::Effect).unwrap();
}

#[test]
fn a_card_returns_itself_from_its_owners_graveyard_untapped_under_its_owners_control() {
    let (t, mut s, p, mut x) = game();
    let revenant = put(&t, &mut s, &mut x, p, "Test Revenant");
    land_enters(&t, &mut s, &mut x, p);
    assert!(s.triggers().is_empty(), "it works only in a graveyard");
    place(&mut s, revenant, Zone::Graveyard);
    let dead = s.object_ref(revenant);
    land_enters(&t, &mut s, &mut x, p.opponent());
    assert!(s.triggers().is_empty(), "not a land its owner controls");
    land_enters(&t, &mut s, &mut x, p);
    assert_eq!(s.triggers().len(), 1);
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(revenant).zone(), Zone::Battlefield);
    assert_ne!(s.object_ref(revenant), dead, "a new object (CR 400.7)");
    assert_eq!(s.controller(revenant), p);
    assert!(!s.object(revenant).is_tapped());
}

/// The card that triggered left the graveyard before its ability resolved, so it is a new
/// object the ability can't find (CR 400.7) and stays where it went.
#[test]
fn a_card_that_left_its_graveyard_first_is_not_returned() {
    let (t, mut s, p, mut x) = game();
    let revenant = find(&s, &t, p, "Test Revenant");
    place(&mut s, revenant, Zone::Graveyard);
    land_enters(&t, &mut s, &mut x, p);
    assert_eq!(s.triggers().len(), 1);
    place(&mut s, revenant, Zone::Exile);
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(revenant).zone(), Zone::Exile);
}
