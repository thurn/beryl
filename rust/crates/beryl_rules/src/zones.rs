//! Zones ([CR 400](https://yawgatog.com/resources/magic-rules/#R400)): moving an object from
//! one to another, and looking at the cards of a hidden one.

use beryl_card::{Objects, Players};

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::cast;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::stack;
use crate::state::{CounterKind, ObjectRef, Zone};
use crate::zone::Place;

/// Moves an object to a zone, to the top of an ordered one. One that has changed zones since
/// does not move ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object
/// that moves from one zone to another becomes a new object with no memory of, or relation
/// to, its previous existence."
#[inline]
pub fn move_to<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    event::move_one(game.state, game.table, game.sink, object.slot, to, reason)
}

/// Moves a set of objects to one zone simultaneously, each to its owner's
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone."
#[inline]
pub fn move_all<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    let entry = Entry::default();
    event::move_batch(
        game.state, game.table, game.sink, objects, to, reason, entry,
    )
}

/// Returns a spell or a permanent to its owner's hand (CR 400.3). A spell leaves the stack
/// without resolving, or is exiled instead ([`event::exiled_instead`]).
pub fn return_to_hand<L: Layout>(game: &mut Game<'_, L>, object: ObjectRef<L>) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    let to = match game.state.object(object.slot).zone() {
        Zone::Battlefield => Some(Zone::Hand),
        Zone::Stack => cast::unstack(game.state, object, Zone::Hand),
        _ => None,
    };
    match to {
        Some(to) => move_to(game, object, to, Reason::Effect),
        None => Ok(()),
    }
}

/// Puts a permanent card onto the battlefield under `controller`'s control, with the face up
/// that it enters with and any counters it enters with
/// ([CR 110.2a](https://yawgatog.com/resources/magic-rules/#R1102a)): "If an effect instructs
/// a player to put an object onto the battlefield, that object enters the battlefield under
/// that player's control unless the effect states otherwise."
pub fn put_onto_battlefield<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    controller: Seat,
    face: FaceIx,
    counters: Option<(CounterKind, u8)>,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    let entry = Entry {
        controller: Some(controller),
        face,
        counters,
        ..Entry::default()
    };
    let (objects, to) = (Bitboard::single(object.slot), Zone::Battlefield);
    event::move_batch(
        game.state,
        game.table,
        game.sink,
        objects,
        to,
        Reason::Effect,
        entry,
    )
}

/// Puts cards onto the battlefield tapped, each under its owner's control, simultaneously:
/// "Those cards enter tapped."
pub fn put_onto_battlefield_tapped<L: Layout>(
    game: &mut Game<'_, L>,
    cards: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let entry = Entry {
        tapped: true,
        ..Entry::default()
    };
    let (to, reason) = (Zone::Battlefield, Reason::Effect);
    event::move_batch(game.state, game.table, game.sink, cards, to, reason, entry)
}

/// `player` looks at the cards in `cards` and from then on knows them (architecture §13): a
/// library and a hand are hidden zones
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "zones in which not all
/// players can be expected to see the cards' faces."
#[inline]
pub fn look_at<L: Layout>(game: &mut Game<'_, L>, player: Seat, cards: Bitboard<L::Slot>) {
    game.state.look(game.sink, player, cards);
}

/// [`Effect::LookAt`](beryl_card::Effect::LookAt): "[Player] looks at [cards]" (CR 400.2):
/// [`look_at`], for each player. The outcome is the cards.
pub(crate) fn look_at_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    cards: Objects,
) -> Outcome<L> {
    let cards = run.scene().objects(cards);
    for player in run.in_turn_order(players) {
        look_at(run.game(), player, cards);
    }
    Outcome::acted_on(cards)
}

/// [`Effect::ReturnToHand`](beryl_card::Effect::ReturnToHand): "Return [objects] to their
/// owners' hands." The outcome is the cards that are there.
pub(crate) fn return_to_hand_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    put_instruction(run, objects, Zone::Hand, Place::Top)
}

/// [`Effect::PutIntoGraveyard`](beryl_card::Effect::PutIntoGraveyard): "Put [objects] into
/// their owners' graveyards" (CR 400.3), each in the order the game lists them. The outcome
/// is the cards it moved, wherever a replacement effect put them: what the effect goes on
/// to say of "it" is said of the card, not of where it went.
pub(crate) fn put_into_graveyard_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let objects = run.elsewhere_than(objects, Zone::Graveyard);
    if objects.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(objects)?;
    // A spell among them leaves the stack without resolving, and one that is exiled
    // instead has moved already.
    let rest = stack::unstack(run.game(), objects, Zone::Graveyard)?;
    let mut moved = objects & !rest;
    for slot in run.state().in_candidate_order(rest) {
        let object = run.state().object_ref(slot);
        move_to(run.game(), object, Zone::Graveyard, Reason::Effect)?;
        if !run.state().is_current(object) {
            moved.insert(slot);
        }
    }
    Ok(Outcome::acted_on(moved))
}

/// Puts the objects `objects` names into their owners' hands or libraries, at `place` in a
/// library (CR 400.3): the arm of [`return_to_hand_instruction`] and of
/// [`library::put_instruction`](crate::library::put_instruction). A spell among them leaves
/// the stack without resolving. The outcome is the cards that are there.
pub(crate) fn put_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    to: Zone,
    place: Place,
) -> Result<Outcome<L>, Full> {
    let objects = run.elsewhere_than(objects, to);
    if objects.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(objects)?;
    let game = run.game();
    let rest = stack::unstack(game, objects, to)?;
    let (reason, entry) = (Reason::Effect, Entry::default());
    event::move_batch_to(
        game.state, game.table, game.sink, rest, to, place, reason, entry,
    )?;
    Ok(run.now_in(objects, to))
}
