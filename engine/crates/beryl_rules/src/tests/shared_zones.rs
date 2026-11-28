//! The zone-change pipeline in a game whose players share a library and a graveyard: a card
//! that leaves either has no owner, so each move that takes one names the player who takes it.

use crate::bitboard::Bitboard;
use crate::ids::Seat;
use crate::profile::Profile;
use crate::replacement::event::{self, Reason};
use crate::state::{State, Zone, ZoneKey};
use crate::zone::Place;
use beryl_kernel::format::Format;
use beryl_kernel::table::GameTable;

use super::events::{CARDS, DECK};

/// A shared game, dealt, with one of its cards put into the shared graveyard.
fn graveyard_card() -> (GameTable, State, crate::ids::Slot) {
    let t = GameTable::new(Format::SHARED_DECK, &CARDS, [DECK, &[]]).unwrap();
    let mut s = State::deal(&t, 7, Profile::EXACT).unwrap();
    let card = *s.library(Seat::P0).last().unwrap();
    s.move_object(&mut event::Sink::off(), card, Zone::Graveyard, Place::Top)
        .unwrap();
    assert!(!s.has_owner(card));
    (t, s, card)
}

#[test]
fn the_player_named_takes_a_card_out_of_the_shared_graveyard() {
    let (t, mut s, card) = graveyard_card();
    let mut sink = event::Sink::off();
    event::move_one_by(
        &mut s,
        &t,
        &mut sink,
        Seat::P1,
        card,
        Zone::Hand,
        Reason::Effect,
    )
    .unwrap();
    assert!(s.zone(ZoneKey::Of(Seat::P1, Zone::Hand)).contains(card));
    assert_eq!(s.object(card).owner(), Some(Seat::P1));
    assert_eq!(sink.taker, None, "the taker lasts for the one move");
    // Back into the shared graveyard, it has no owner again.
    let back = Bitboard::single(card);
    event::move_batch_by(
        &mut s,
        &t,
        &mut sink,
        Seat::P0,
        back,
        Zone::Graveyard,
        Reason::Discard,
        event::Entry::default(),
    )
    .unwrap();
    assert!(!s.has_owner(card));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "with no player named to take it")]
fn a_card_taken_out_of_the_shared_graveyard_by_nobody_panics_in_a_debug_build() {
    let (t, mut s, card) = graveyard_card();
    let mut sink = event::Sink::off();
    let _ = event::move_one(&mut s, &t, &mut sink, card, Zone::Hand, Reason::Effect);
}
