//! A game of the shared-deck format through the zone chokepoints: one library and one graveyard
//! for both players, whose cards have no owner while they are there.

use beryl_card::{CardDef, Seat, Zone};

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::format::Format;
use crate::ids::{LargeSlot, Slot, SlotId};
use crate::layout::{Large, Layout, Small};
use crate::profile::Profile;
use crate::state::{State, ZoneKey};
use crate::table::{CardTable, GameTable};
use crate::zone::Place;

static ALPHA: CardDef = CardDef {
    name: "Alpha",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static BETA: CardDef = CardDef {
    name: "Beta",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static CARDS: CardTable = CardTable::new(&[&ALPHA, &BETA]);

/// An 80-card shared deck, more than one player's deck slots in [`Small`].
fn table() -> GameTable {
    GameTable::new(
        Format::SHARED_DECK,
        &CARDS,
        [&[(40, "Alpha"), (40, "Beta")], &[]],
    )
    .unwrap()
}

fn deal<L: Layout>(t: &GameTable) -> State<L> {
    State::<L>::deal(t, 7, Profile::EXACT).unwrap()
}

/// Both players' sets of the shared zone, which must hold the same cards.
fn shared_set<L: Layout>(s: &State<L>, zone: Zone) -> Bitboard<L::Slot> {
    let set = s.zone(ZoneKey::Of(Seat::P0, zone));
    assert_eq!(set, s.zone(ZoneKey::Of(Seat::P1, zone)));
    set
}

fn unowned<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    let o = s.object(slot);
    o.owner().is_none() && o.base_controller == Seat::P0 && !s.has_owner(slot)
}

#[test]
fn an_eighty_card_shared_deck_is_dealt_into_one_library_in_small() {
    let t = table();
    let s = deal::<Small>(&t);
    assert!(s.shares_zones());
    assert_eq!(s.library(Seat::P0), s.library(Seat::P1));
    assert_eq!(s.library(Seat::P0).len(), 80);
    let slots: Bitboard<Slot> = (0..80).map(Slot::from_index).collect();
    assert_eq!(shared_set(&s, Zone::Library), slots);
    assert!(s.library(Seat::P0).iter().all(|&slot| unowned(&s, slot)));
    let first = s.library(Seat::P0)[0];
    assert!(s.holds(Seat::P0, first) && s.holds(Seat::P1, first));
    assert_eq!(
        s.zone_owners(Zone::Library)
            .in_seat_order()
            .collect::<Vec<_>>(),
        [Seat::P0]
    );
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn a_card_drawn_goes_to_the_drawer_and_comes_back_with_no_owner() {
    let t = table();
    let mut s = deal::<Small>(&t);
    let sink = &mut Sink::off();

    let drawn = s.draw(sink, Seat::P1).unwrap().unwrap();
    assert_eq!(s.object(drawn).owner(), Some(Seat::P1));
    assert_eq!(s.object(drawn).zone, Zone::Hand);
    assert!(s.zone(ZoneKey::Of(Seat::P1, Zone::Hand)).contains(drawn));
    assert!(!shared_set(&s, Zone::Library).contains(drawn));
    assert_eq!(s.library(Seat::P0).len(), 79);
    assert!(s.knows(Seat::P1, drawn) && !s.knows(Seat::P0, drawn));
    assert_eq!(s.check_invariants(&t), Ok(()));

    s.move_object(sink, drawn, Zone::Graveyard, Place::Top)
        .unwrap();
    assert!(unowned(&s, drawn));
    assert!(shared_set(&s, Zone::Graveyard).contains(drawn));
    assert_eq!(s.graveyard(Seat::P0), &[drawn]);
    assert_eq!(s.graveyard(Seat::P1), &[drawn]);
    assert!(s.zone(ZoneKey::Of(Seat::P1, Zone::Hand)).is_empty());
    assert_eq!(s.check_invariants(&t), Ok(()));

    s.move_object(sink, drawn, Zone::Library, Place::Top)
        .unwrap();
    assert!(unowned(&s, drawn));
    assert_eq!(s.library_from_top(Seat::P1).next(), Some(drawn));
    assert!(shared_set(&s, Zone::Library).contains(drawn));
    assert!(shared_set(&s, Zone::Graveyard).is_empty());
    assert_eq!(s.library(Seat::P0).len(), 80);
    assert_eq!(s.check_invariants(&t), Ok(()));

    // The opponent draws the card they saw go back.
    let again = s.draw(sink, Seat::P0).unwrap().unwrap();
    assert_eq!(again, drawn);
    assert_eq!(s.object(again).owner(), Some(Seat::P0));
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn a_claim_gives_a_shared_card_to_a_player_until_it_is_disowned() {
    let t = table();
    let mut s = deal::<Small>(&t);
    let top = s.library_from_top(Seat::P0).next().unwrap();
    s.claim(top, Seat::P1);
    assert_eq!(s.object(top).owner(), Some(Seat::P1));
    assert_eq!(s.object(top).base_controller, Seat::P1);
    s.disown(top);
    assert!(unowned(&s, top));
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn a_shuffle_rebinds_every_card_of_the_shared_library() {
    let t = table();
    let mut s = deal::<Small>(&t);
    let sink = &mut Sink::off();
    let drawn = s.draw(sink, Seat::P0).unwrap().unwrap();
    s.move_object(sink, drawn, Zone::Library, Place::Top)
        .unwrap();
    assert!(s.knows(Seat::P0, drawn));
    let before = s.library(Seat::P0).to_vec();
    s.shuffle_library(Seat::P1).unwrap();
    assert_eq!(s.library(Seat::P0).len(), 80);
    assert!(!s.knows(Seat::P0, drawn));
    let mut after = s.library(Seat::P0).to_vec();
    let mut sorted = before.clone();
    after.sort_unstable();
    sorted.sort_unstable();
    assert_eq!(after, sorted);
    let alphas = |s: &State| {
        (s.library(Seat::P0).iter())
            .filter(|&&slot| s.object(slot).card.index() == 0)
            .count()
    };
    assert_eq!(alphas(&s), 40);
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn a_restarted_shared_game_is_shared() {
    let t = table();
    let mut s = deal::<Small>(&t);
    let sink = &mut Sink::off();
    let drawn = s.draw(sink, Seat::P1).unwrap().unwrap();
    let milled = s.library_from_top(Seat::P0).next().unwrap();
    s.move_object(sink, milled, Zone::Graveyard, Place::Top)
        .unwrap();
    s.restart(&t, Seat::P1, Bitboard::EMPTY).unwrap();
    assert!(s.shares_zones());
    assert_eq!(s.library(Seat::P0).len(), 80);
    assert_eq!(s.library(Seat::P0), s.library(Seat::P1));
    assert!(shared_set(&s, Zone::Library).contains(drawn));
    assert!(shared_set(&s, Zone::Graveyard).is_empty());
    assert!(s.library(Seat::P0).iter().all(|&slot| unowned(&s, slot)));
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn widening_keeps_a_shared_game_shared_and_each_card_at_its_place() {
    let t = table();
    let mut s = deal::<Small>(&t);
    let sink = &mut Sink::off();
    let drawn = s.draw(sink, Seat::P1).unwrap().unwrap();
    s.move_object(sink, drawn, Zone::Graveyard, Place::Top)
        .unwrap();
    let wide = s.widen();
    assert!(wide.shares_zones());
    assert_eq!(wide.library(Seat::P0).len(), 79);
    assert_eq!(wide.library(Seat::P0), wide.library(Seat::P1));
    assert_eq!(wide.graveyard(Seat::P1), &[LargeSlot::from(drawn)]);
    // The shared deck's 61st card keeps its place, the first of the second player's slots.
    assert_eq!(LargeSlot::shared_deck(60), LargeSlot::from(Slot::new(60)));
    assert_eq!(wide.check_invariants(&t), Ok(()));

    // A shared game dealt in Large is the one Small deals, widened.
    let fresh = deal::<Small>(&t).widen();
    assert_eq!(deal::<Large>(&t).first_difference(&fresh), None);
}

#[test]
fn a_subgame_of_a_shared_game_is_dealt_from_the_shared_library_once() {
    let t = table();
    let s = deal::<Large>(&t);
    let inner = s.deal_subgame(Format::SHARED_DECK).unwrap();
    assert!(inner.shares_zones());
    assert_eq!(inner.library(Seat::P0).len(), 80);
    assert_eq!(inner.library(Seat::P0), inner.library(Seat::P1));
    assert_eq!(shared_set(&inner, Zone::Library).count(), 80);
    assert!(
        inner
            .library(Seat::P0)
            .iter()
            .all(|&slot| unowned(&inner, slot))
    );
    assert_eq!(inner.check_invariants(&t), Ok(()));
}
