//! Phasing ([CR 702.26]): what phases out with a permanent, what an Equipment attached to a
//! permanent that left the battlefield phases in attached to, and whose untap step phases a
//! permanent in.
//!
//! [CR 702.26]: https://yawgatog.com/resources/magic-rules/#R70226

use super::cast::Game;
use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::ids::{CardId, CardIx};
use crate::profile::Profile;
use crate::state::{Continuous, Duration, LayerChange, State, TurnStep, Zone, ZoneKey};
use crate::table::GameTable;

use crate::zone::Place;
use beryl_card::FaceIx;

/// Brings the layer results up to date and checks every invariant of the state.
fn settled(g: &mut Game) {
    crate::layers::refresh(&mut g.s, &g.t, &mut g.x.events).unwrap();
    check(&g.s, &g.t);
}

fn check<L: crate::layout::Layout>(s: &State<L>, t: &GameTable) {
    if let Err(violation) = s.check_invariants(t) {
        panic!("{violation:?}");
    }
}

/// The rule [CR 702.26g]: "When a permanent phases out, any Auras, Equipment, or Fortifications attached
/// to that permanent phase out at the same time. ... An Aura, Equipment, or Fortification that
/// phased out indirectly won't phase in by itself, but instead phases in along with the
/// permanent it's attached to." The artifact P1 controls phases out with P0's creature, and
/// phases in at P0's untap step, attached, not at P1's.
///
/// [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
#[test]
fn what_is_attached_phases_out_and_in_with_its_permanent() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Battlefield);
    let crypt = g.give(g.them, "Test Crypt", Zone::Battlefield);
    g.s.board().attach(crypt, bear);
    settled(&mut g);

    g.s.phase_out(Bitboard::single(bear)).unwrap();
    settled(&mut g);
    let both = Bitboard::single(bear) | Bitboard::single(crypt);
    assert_eq!(g.s.phased_out(), both);
    assert!(g.s.in_zone(Zone::Battlefield).is_empty());
    assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
    assert!(g.s.phasing_in_for(g.them).is_empty());
    assert_eq!(g.s.phasing_in_for(g.me), both);

    g.s.phase_in(both).unwrap();
    settled(&mut g);
    assert_eq!(
        g.s.zone(ZoneKey::Of(g.me, Zone::Battlefield)),
        Bitboard::single(bear)
    );
    assert_eq!(g.s.object(crypt).attached_to(), Some(bear));
}

/// The rule [CR 702.26i]: "An Aura, Equipment, or Fortification that phased out directly will phase in
/// attached to the object or player it was attached to when it phased out, if that object is
/// still in the same zone or that player is still in the game. If not, that Aura, Equipment, or
/// Fortification phases in unattached." The phased-out artifact is unattached as the creature
/// it was on leaves the battlefield, before the slot can hold another object.
///
/// [CR 702.26i]: https://yawgatog.com/resources/magic-rules/#R70226i
#[test]
fn what_phased_out_alone_phases_in_unattached_once_its_permanent_left() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    let crypt = g.give(g.me, "Test Crypt", Zone::Battlefield);
    g.s.board().attach(crypt, bear);
    settled(&mut g);

    g.s.phase_out(Bitboard::single(crypt)).unwrap();
    settled(&mut g);
    assert_eq!(g.s.phased_out(), Bitboard::single(crypt));
    assert_eq!(g.s.object(crypt).attached_to(), Some(bear));

    g.s.move_object(&mut Sink::off(), bear, Zone::Graveyard, Place::Top)
        .unwrap();
    settled(&mut g);
    assert_eq!(g.s.object(crypt).attached_to(), None);

    g.s.phase_in(g.s.phasing_in_for(g.me)).unwrap();
    settled(&mut g);
    assert!(g.s.phased_out().is_empty());
    assert!(
        g.s.zone(ZoneKey::Of(g.me, Zone::Battlefield))
            .contains(crypt)
    );
}

/// The rule [CR 502.1]: "all phased-out permanents that the active player controlled when they phased
/// out phase in." P0 controls P1's creature through a control-changing effect as it phases out;
/// with the effect ended, it still phases in at P0's untap step (the ruling of Teferi's
/// Protection: "if the duration of the control-change effect expires before it phases in, that
/// permanent phases in under that other player's control as your next untap step begins"). The
/// phased-out state survives the move to the wide layout and back.
///
/// [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
#[test]
fn a_permanent_phases_in_for_who_controlled_it_as_it_phased_out() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    let source = CardIx::FIRST;
    let stolen = Continuous::Layer(g.s.object_ref(bear), LayerChange::Control(g.me));
    g.s.start_effect(source, stolen, Duration::EndOfTurn)
        .unwrap();
    settled(&mut g);
    assert_eq!(g.s.controller(bear), g.me);

    g.s.phase_out(Bitboard::single(bear)).unwrap();
    g.s.expire(Duration::EndOfTurn);
    settled(&mut g);
    assert_eq!(g.s.phasing_in_for(g.me), Bitboard::single(bear));
    assert!(g.s.phasing_in_for(g.them).is_empty());

    let wide = g.s.widen();
    check(&wide, &g.t);
    assert_eq!(wide.phased_out().count(), 1);

    g.s.phase_in(Bitboard::single(bear)).unwrap();
    settled(&mut g);
    assert_eq!(g.s.controller(bear), g.them);
    assert!(g.s.memos(bear).iter().next().is_none());
}

/// A turn-start checkpoint records each permanent by its own card, its counters and what it is
/// attached to, so a board with a phased-out permanent, or with a merged permanent whose other
/// cards are in no zone the checkpoint lists ([CR 731.2]): "That permanent becomes a merged
/// permanent represented by the card or copy that represented that object in addition to any
/// other components that were representing it", is reported as one no game may be rebuilt
/// from ([`State::off_board`]).
///
/// [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312
#[test]
fn a_board_with_a_phased_out_or_merged_permanent_is_not_rebuilt() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Battlefield);
    let other = g.give(g.me, "Test Bear", Zone::Hand);
    settled(&mut g);
    g.s.board().set_step(TurnStep::Upkeep);
    assert_eq!(g.s.off_board(&g.t), None);

    let (card, onto) = (g.s.object_ref(other), g.s.object_ref(bear));
    g.s.merge(&mut Sink::off(), card, FaceIx::FRONT, onto, true)
        .unwrap();
    settled(&mut g);
    assert_eq!(
        g.s.off_board(&g.t).as_deref(),
        Some("Test Bear merged with other cards")
    );

    g.s.phase_out(Bitboard::single(bear)).unwrap();
    settled(&mut g);
    assert_eq!(g.s.off_board(&g.t).as_deref(), Some("Test Bear phased out"));
}
