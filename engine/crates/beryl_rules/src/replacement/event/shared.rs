//! The zone-change pipeline in a game whose players share a library and a graveyard (the
//! shared-deck format): a card there has no owner, so each move that takes one out names the
//! player who takes it, and the pipeline makes that player its owner.
//!
//! Implements: none. The shared-deck format's rules for the pipeline [`super`] runs.

use super::{
    Entry, MoveReasons, PreparedMove, Reason, Sink, move_batch_to, move_batch_with_reasons,
    move_one, zone_change,
};
use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::stack::trigger;
use crate::state::{State, Zone};
use crate::table::GameTable;
use crate::zone::Place;

/// [`move_one`] by `player`, who takes the object if it leaves a shared library or graveyard
/// ([`taken`]).
// Always inline: the move of every game of another format runs through here, and
// the test of the format must cost it no call.
#[inline(always)]
#[allow(clippy::inline_always)]
pub(crate) fn move_one_by<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    if s.shares_zones() {
        let (objects, entry) = (Bitboard::single(slot), Entry::default());
        return taken(s, t, sink, player, objects, to, Place::Top, reason, entry);
    }
    move_one(s, t, sink, slot, to, reason)
}

/// [`move_batch`](super::move_batch) by `player`, who takes the objects that leave a shared
/// library or graveyard ([`taken`]).
// Always inline: the move of every game of another format runs through here, and
// the test of the format must cost it no call.
#[inline(always)]
#[allow(clippy::inline_always, clippy::too_many_arguments)]
pub(crate) fn move_batch_by<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    objects: Bitboard<L::Slot>,
    to: Zone,
    reason: Reason,
    entry: Entry<L>,
) -> Result<(), Full> {
    move_batch_to_by(s, t, sink, player, objects, to, Place::Top, reason, entry)
}

/// [`move_batch_to`] by `player`, who takes the objects that leave a shared library or
/// graveyard ([`taken`]).
// Always inline: the move of every game of another format runs through here, and
// the test of the format must cost it no call.
#[inline(always)]
#[allow(clippy::inline_always, clippy::too_many_arguments)]
pub(crate) fn move_batch_to_by<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    objects: Bitboard<L::Slot>,
    to: Zone,
    place: Place,
    reason: Reason,
    entry: Entry<L>,
) -> Result<(), Full> {
    if s.shares_zones() {
        return taken(s, t, sink, player, objects, to, place, reason, entry);
    }
    move_batch_to(s, t, sink, objects, to, place, reason, entry)
}

/// Moves `objects` in a game whose players share a library and a graveyard, with `player` as
/// the taker of those that leave a shared zone ([`Sink::taker`]) for the length of the batch.
/// The shared-deck format gives such a card to the player who takes it: "The 'owner' of a card
/// on the stack or permanent on the battlefield is considered to be the person who cast it."
///
/// Only these moves run the pipeline's shared steps ([`super::move_ordered`] with `SHARED`): a
/// test of the format in every batch would cost a game of any other format more than all of
/// them, so a move that names no taker runs the steps every game runs. Every caller that can
/// take a card from a library or a graveyard names its taker, and a debug build panics at a
/// batch that takes one without ([`assert_none_taken`]).
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn taken<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    objects: Bitboard<L::Slot>,
    to: Zone,
    place: Place,
    reason: Reason,
    entry: Entry<L>,
) -> Result<(), Full> {
    let before = sink.taker.replace(player);
    let reasons = MoveReasons {
        ordinary: reason,
        sacrifices: None,
    };
    let result = move_batch_with_reasons::<L, true>(s, t, sink, objects, to, place, reasons, entry);
    sink.taker = before;
    result
}

/// Panics, in a game whose players share a library and a graveyard, if any of `objects` is in a
/// shared zone: a batch that names no taker has nobody to give such a card to. A debug build
/// checks every batch that names none.
#[cold]
#[inline(never)]
pub(super) fn assert_none_taken<L: Layout>(s: &State<L>, objects: Bitboard<L::Slot>) {
    if !s.shares_zones() {
        return;
    }
    let untaken = objects.filter(|slot| !s.has_owner(slot));
    assert!(
        untaken.is_empty(),
        "a card leaves a shared library or graveyard with no player named to take it"
    );
}

/// Claims each card of `ordered` that is in a shared library or graveyard for the player who
/// takes it, before the batch is prepared: the player under whose control it enters the
/// battlefield, or else the batch's taker ([`Sink::taker`]). So everything that prepares the
/// move and reads the owner reads the taker.
pub(super) fn claim_taken<L: Layout>(
    s: &mut State<L>,
    sink: &Sink<L>,
    ordered: &[L::Slot],
    entry: Entry<L>,
) {
    let taker = entry
        .controller
        .or(sink.taker)
        .expect("a batch that names its taker");
    for &slot in ordered {
        if !s.has_owner(slot) {
            s.claim(slot, taker);
        }
    }
}

/// Gives back the cards of `ordered` that [`claim_taken`] claimed and that a replacement effect
/// kept in their shared zone: they have no owner again. A card still in a shared library or
/// graveyard with an owner is one that was claimed.
#[cold]
#[inline(never)]
pub(super) fn disown_unmoved<L: Layout>(
    s: &mut State<L>,
    ordered: &[L::Slot],
    moves: &[PreparedMove<L>],
) {
    for &slot in ordered {
        let claimed = s.in_shared_zone(slot) && s.has_owner(slot);
        if claimed && !moves.iter().any(|&(moved, ..)| moved == slot) {
            s.disown(slot);
        }
    }
}

/// The player who discards each card of `moves` that leaves a hand to be discarded: its owner,
/// read before the move, since a card that enters a shared graveyard has none after it.
#[cold]
#[inline(never)]
pub(super) fn discarders<L: Layout, const N: usize>(
    s: &State<L>,
    moves: &[PreparedMove<L>],
    reasons: MoveReasons<'_, L::Slot>,
) -> L::Queue<(L::Slot, Seat), N> {
    let mut discarders: L::Queue<(L::Slot, Seat), N> = Default::default();
    for &(slot, from, ..) in moves {
        if from == Zone::Hand && reasons.for_slot(slot) == Reason::Discard {
            if let Some(owner) = s.object(slot).owner() {
                discarders.push_bounded((slot, owner));
            }
        }
    }
    discarders
}

/// Matches the triggers of the moved batch in a game whose zones are shared, as
/// [`move_ordered`] does in every other. A "whenever a player discards a card" trigger reads
/// the discarder as the card's owner ([`trigger::match_event`]), and a discarded card in the
/// shared graveyard has no owner, so it is given to its discarder while its event is matched
/// and has no owner again after.
#[cold]
#[inline(never)]
pub(super) fn match_shared<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    moves: &[PreparedMove<L>],
    discarders: &[(L::Slot, Seat)],
    reasons: MoveReasons<'_, L::Slot>,
    place: Place,
) -> Result<(), Full> {
    for &(slot, from, dest, _) in moves {
        let event = zone_change(reasons, place, slot, s.object_ref(slot), from, dest);
        let discarder = discarders
            .iter()
            .find(|&&(discarded, _)| discarded == slot)
            .map(|&(_, player)| player);
        match discarder {
            Some(player) if !s.has_owner(slot) => {
                s.claim(slot, player);
                let matched = trigger::match_event(s, t, &event);
                s.disown(slot);
                matched?;
            }
            _ => trigger::match_event(s, t, &event)?,
        }
    }
    Ok(())
}
